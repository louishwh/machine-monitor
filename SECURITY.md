# Security Policy

FleetWatch is a remote-monitoring and remote-control system. Whoever controls
the console master key can, on machines where it is enabled, run commands on
your fleet. Please take the model and the limitations below seriously.

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Please report privately via **GitHub Security Advisories**
(repository → *Security* → *Report a vulnerability*). If you cannot use that,
email `security@<SET-ME>` *(maintainer: replace this placeholder with a real
contact address before publishing)*.

We aim to acknowledge reports within a few days and will coordinate a fix and
disclosure timeline with you.

## Supported versions

FleetWatch is pre-1.0. Only the latest `main` is supported with security fixes.

| Version | Supported |
|---------|-----------|
| `main` (latest) | ✅ |
| older commits / pre-releases | ❌ |

## Security model

- **Trust root: the console master key.** The console / `fwctl` holds an ed25519
  master key. The private key never leaves the operator's machine. The server
  never mints credentials — it only verifies signatures.
- **One-time pairing (TOFU).** On first pairing the server registers the
  console's public key (presented with the one-time `pairing_token`) and stores
  it. After that the server is "paired" and ignores the token.
- **Signed control plane.** Every `/api/*` request is ed25519-signed by the
  master key and carries a timestamp; the server rejects requests outside a
  ±300-second window. There is no anonymous access and no web UI on the control
  plane.
- **Agent identity tokens.** Each agent authenticates with a console-signed
  identity token (issued per machine via `fwctl issue` / the console). Tokens can
  be revoked individually; revocation kicks the agent offline and rejects
  reconnects.
- **TLS with pinned self-signed cert.** The server serves TLS (self-signed by
  default). Agents pin the server certificate at enrollment.
- **Shell is off by default.** Controlled shell access is disabled per machine
  and must be explicitly enabled; all commands and output are audited.

## Known limitations & hardening guidance

These are documented honestly — several came out of a real security audit.

1. **Pairing token is the takeover surface.** `pairing_token` must be a strong
   random value (e.g. `openssl rand -hex 16`). The server **refuses to boot**
   while unpaired if the token is the default `changeme` or shorter than 16
   chars. If `/api/pair` is reachable from the public internet before you have
   paired, anyone with the token (or a weak/guessable one) can register their
   own key and take over the control plane. Pair before exposing the server, and
   keep the token secret.
2. **The master key is full control, including RCE.** Whoever holds the console
   master key can do anything the control plane allows — including remote shell
   (remote code execution) on any machine where shell has been explicitly
   enabled (shell is OFF by default per machine). Protect the key: it lives at
   `~/.fleetwatch/master.key` (mode `0600`) and/or in the macOS keychain. Treat
   it like an SSH CA key.
3. **No key rotation / revocation of the console key yet.** Rotating or revoking
   a compromised master key is not yet supported (planned). If the key leaks you
   currently have to re-pair from scratch.
4. **Signature does not cover the query string.** The control-plane signature
   currently covers method + path + body but **not** the query string. There is
   no nonce, so replay is bounded only by the ±300-second timestamp window.
   Prefer carrying parameters in the body where it matters.
5. **Reverse-proxy TLS hop.** If you front the server with a reverse proxy that
   terminates TLS, the proxy→server hop may use `tls_insecure_skip_verify`.
   Where possible, pin the server cert on that hop instead of skipping
   verification, or keep proxy↔server on a trusted local network.
6. **Audit log is plaintext SQLite.** The audit log stores commands and their
   output in plaintext in the SQLite database (`db_path`). Protect the DB file
   (filesystem permissions, disk encryption) — it may contain sensitive output.
