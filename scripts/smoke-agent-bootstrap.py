#!/usr/bin/env python3
"""Verify the public one-line installer on an ephemeral Ubuntu/systemd runner.

Uses an isolated signing key, local TLS server and hidden PTY input. Never
prints the test identity; real production credentials are not required.
"""
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import shlex
import signal
import socket
import ssl
import stat
import subprocess
import tempfile
import time
import tomllib
import urllib.request
import uuid

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


SITE = "https://blog.louishwh.tech/machine-monitor"


def run(*args, **kwargs):
    return subprocess.run(args, check=True, stdin=subprocess.DEVNULL, **kwargs)


def interactive_install(command, server_url, token):
    pid, fd = pty.fork()
    if pid == 0:
        os.execv("/bin/sh", ["sh", "-c", command])
    output = bytearray()
    answered_server = answered_token = False
    status = None
    deadline = time.monotonic() + 240
    try:
        while time.monotonic() < deadline:
            if select.select([fd], [], [], 0.25)[0]:
                try:
                    chunk = os.read(fd, 8192)
                except OSError:
                    break
                if not chunk:
                    break
                output.extend(chunk)
                if not answered_server and b"FleetWatch server URL" in output:
                    os.write(fd, (server_url + "\n").encode())
                    answered_server = True
                if not answered_token and b"Machine identity token:" in output:
                    os.write(fd, (token + "\n").encode())
                    answered_token = True
            done, candidate = os.waitpid(pid, os.WNOHANG)
            if done:
                status = candidate
                break
        else:
            os.killpg(pid, signal.SIGKILL)
            raise RuntimeError("one-line installer exceeded its test deadline")
        if status is None:
            _, status = os.waitpid(pid, 0)
    finally:
        os.close(fd)
    assert token.encode() not in output, "identity token appeared in installer output"
    if os.waitstatus_to_exitcode(status) != 0:
        raise RuntimeError(output.decode(errors="replace")[-6000:])
    assert answered_server and answered_token, "installer did not request hidden enrollment input"


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.geteuid() != 0:
        raise SystemExit("Run only as root on an ephemeral GitHub Actions runner.")
    run("systemctl", "show", "--property=Version", "--value", stdout=subprocess.DEVNULL)
    with tempfile.TemporaryDirectory(prefix="fleetwatch-bootstrap-") as work:
        work = Path(work)
        installer = work / "install.sh"
        query = os.environ.get("GITHUB_REF_NAME", "verify")
        run("curl", "-fsSL", "--proto", "=https", f"{SITE}/install.sh?verify={query}", "-o", str(installer))
        run("sh", str(installer), "server")
        signing_key = Ed25519PrivateKey.generate()
        pubkey = signing_key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        machine_id = str(uuid.uuid4())
        payload = json.dumps({"machine_id": machine_id, "name": "bootstrap-smoke", "issued_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}, separators=(",", ":")).encode()
        token = base64.b64encode(payload).decode() + "." + base64.b64encode(signing_key.sign(payload)).decode()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        origin = f"https://127.0.0.1:{port}"
        cert = work / "cert.pem"
        config = work / "server.toml"
        values = {
            "bind": f"127.0.0.1:{port}",
            "console_public_key_b64": base64.b64encode(pubkey).decode(),
            "db_path": str(work / "server.db"),
            "tls_cert_path": str(cert),
            "tls_key_path": str(work / "server.key"),
        }
        config.write_text("\n".join(f"{key} = {json.dumps(value)}" for key, value in values.items()))
        log = (work / "server.log").open("wb")
        server = subprocess.Popen(["/usr/bin/fleetwatch-server"], env={**os.environ, "FW_SERVER_CONFIG": str(config)}, stdout=log, stderr=log)
        try:
            for _ in range(100):
                if cert.exists():
                    try:
                        context = ssl.create_default_context(cafile=str(cert))
                        urllib.request.urlopen(origin + "/health", context=context, timeout=1).close()
                        break
                    except (OSError, ValueError):
                        pass
                time.sleep(0.1)
            else:
                raise RuntimeError("test server did not become ready")
            public_ca = base64.b64encode(cert.read_bytes()).decode()
            command = f"curl -fsSL {shlex.quote(SITE + '/install.sh?verify=' + query)} | sh -s -- agent --server-ca-base64 {shlex.quote(public_ca)}"
            interactive_install(command, origin, token)
            agent_config = Path("/etc/fleetwatch/agent.toml")
            saved = agent_config.read_bytes()
            assert stat.S_IMODE(agent_config.stat().st_mode) == 0o600
            assert tomllib.loads(saved.decode())["identity_token"] == token
            run("systemctl", "is-enabled", "--quiet", "fleetwatch-agent.service")
            run("systemctl", "is-active", "--quiet", "fleetwatch-agent.service")

            def signed_get(path):
                timestamp = datetime.datetime.now(datetime.timezone.utc).isoformat()
                canonical = f"GET\n{path}\n{timestamp}\n{hashlib.sha256(b'').hexdigest()}".encode()
                request = urllib.request.Request(origin + path, headers={"x-fw-timestamp": timestamp, "x-fw-signature": base64.b64encode(signing_key.sign(canonical)).decode()})
                with urllib.request.urlopen(request, context=context, timeout=5) as response:
                    return json.load(response)

            for _ in range(100):
                if any(row["id"] == machine_id and row["online"] for row in signed_get("/api/machines")):
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError("started Agent did not become online")
            print("One-line hidden-input install, TLS enrollment, systemd startup and online status: OK")

            invalid = work / "invalid-token"
            invalid.write_text("invalid-identity")
            result = subprocess.run(["sh", str(installer), "agent", "--server", origin, "--identity-file", str(invalid), "--server-ca", str(cert)], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            assert result.returncode != 0, "invalid identity unexpectedly accepted"
            assert agent_config.read_bytes() == saved, "failed enrollment overwrote working config"
            run("systemctl", "is-active", "--quiet", "fleetwatch-agent.service")
            journal = subprocess.check_output(["journalctl", "-u", "fleetwatch-agent", "--no-pager"])
            assert token.encode() not in journal and token.encode() not in result.stdout
            print("Invalid token rejected; existing config and running service preserved; token absent from logs: OK")

            token_file = work / "identity-token"
            token_file.write_text(token)
            token_file.chmod(0o600)
            shim = work / "shim"
            shim.mkdir()
            systemctl = shim / "systemctl"
            systemctl.write_text("#!/bin/sh\nif [ \"$1\" = restart ]; then echo 'simulated startup failure' >&2; exit 42; fi\nexec /usr/bin/systemctl \"$@\"\n")
            systemctl.chmod(0o700)
            result = subprocess.run(["sh", str(installer), "agent", "--server", origin, "--identity-file", str(token_file), "--server-ca", str(cert)], env={**os.environ, "PATH": str(shim) + ":" + os.environ["PATH"]}, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            assert result.returncode != 0 and b"simulated startup failure" in result.stdout
            assert b"Agent is running and enabled" not in result.stdout
            assert token.encode() not in result.stdout
            assert stat.S_IMODE(agent_config.stat().st_mode) == 0o600
            print("File-input enrollment verified; service startup failure reported without a false success: OK")
        finally:
            subprocess.run(["systemctl", "stop", "fleetwatch-agent.service"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.run(["systemctl", "disable", "fleetwatch-agent.service"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
            log.close()


if __name__ == "__main__":
    main()
