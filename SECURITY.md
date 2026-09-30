# Security Policy

FleetWatch is a remote-monitoring and remote-control system. Whoever controls
the console master key can, on machines where it is enabled, run commands on
your fleet. Please take the model and the limitations below seriously.

## Reporting a vulnerability

**Do not open a public issue for security problems.**

Please report privately via **GitHub Security Advisories**
(repository → *Security* → *Report a vulnerability*).

We aim to acknowledge reports within a few days and will coordinate a fix and
disclosure timeline with you.

## Supported versions

FleetWatch is pre-1.0. Only the latest `main` is supported with security fixes.

| Version | Supported |
|---------|-----------|
| `main` (latest) | ✅ |
| older commits / pre-releases | ❌ |

## Security model

- **Trust root: the console master key.** The console stores its ed25519 private
  key in the OS keychain; `fwctl` stores it in a local file. If `ops-agent` is
  deployed, it currently needs a copy of that same key. The server never mints
  credentials — it only verifies signatures.
- **One-time pairing (TOFU).** On first pairing the server registers the
  console's public key (presented with the one-time `pairing_token`) and stores
  it. After that the server is "paired" and ignores the token.
- **Signed control plane.** Protected `/api/*` requests are ed25519-signed by
  the master key and carry a timestamp; the server rejects requests outside a
  ±300-second window. The one-time `/api/pair` endpoint uses its pairing token
  before the public key is registered. `/health` is public.
- **Agent identity tokens.** Each agent authenticates with a console-signed
  identity token (issued per machine via `fwctl issue` / the console). Tokens can
  be revoked individually; revocation kicks the agent offline and rejects
  reconnects.
- **TLS with pinned self-signed cert.** The server serves TLS (self-signed by
  default). Agents pin the server certificate at enrollment.
- **Shell is off by default.** Controlled shell access is disabled per machine
  and must be explicitly enabled. Requests are recorded before dispatch;
  completed results are added to the audit log when available.

## Known limitations & hardening guidance

These are documented honestly — several came out of a real security audit.

1. **Pairing token is the takeover surface.** `pairing_token` must be a strong
   random value (e.g. `openssl rand -hex 16`). The server **refuses to boot**
   while unpaired if the token is the default `changeme` or shorter than 16
   chars. If `/api/pair` is reachable from the public internet before you have
   paired, anyone with the token (or a weak/guessable one) can register their
   own key and take over the control plane. Pair before exposing the server, and
   keep the token secret.
   Use `fwctl pair --pairing-token-prompt` so the token does not enter Shell
   history or process arguments.
2. **The master key is full control, including RCE.** Whoever holds the console
   master key can do anything the control plane allows — including remote shell
   (remote code execution) on any machine where shell has been explicitly
   enabled (shell is OFF by default per machine). Protect the key: it lives at
   `~/.fleetwatch/master.key` (mode `0600`) and/or in the macOS keychain. Treat
   it like an SSH CA key. **`ops-agent` is GET-only by its own implementation,
   but the server does not limit that copied master key to GET.** Do not deploy
   `ops-agent` on a less trusted host until it has a separate scoped identity.
3. **No key rotation / revocation of the console key yet.** Rotating or revoking
   a compromised master key is not yet supported (planned). If the key leaks you
   currently have to re-pair from scratch.
4. **Replay window.** The control-plane signature covers method, path, query,
   timestamp and body hash. There is no nonce, so a captured request could be
   replayed within the ±300-second timestamp window if TLS or a trusted proxy
   were compromised.
5. **Reverse-proxy TLS hop.** If you front the server with a reverse proxy that
   terminates TLS, the proxy→server hop may use `tls_insecure_skip_verify`.
   Where possible, pin the server cert on that hop instead of skipping
   verification, or keep proxy↔server on a trusted local network.
6. **Audit log is plaintext SQLite.** The audit log stores commands and their
   output in plaintext in the SQLite database (`db_path`). The packaged service
   restricts its state directory and files; custom deployments must also set
   private permissions and consider disk encryption.
7. **Agent identities are bearer tokens.** `/etc/fleetwatch/agent.toml` contains
   the identity token in plaintext. The package and `enroll` command restrict
   it to mode `0600`; protect backups, terminal output, and enrollment commands
   containing that token. Use `--identity-prompt` when enrolling so the token
   does not enter process arguments or Shell history. Revoke a token if exposed.
8. **APT signing key in Actions.** The APT repository's private signing key is
   stored as the `GPG_PRIVATE_KEY` Actions secret and imported only in the
   release job. The workflow publishes from this public repository using its
   scoped `GITHUB_TOKEN`. Never commit or print the signing key. Repository
   write access can be used to change a workflow that reads the secret, so
   restrict write access and protect release tags. Keep secure offline backups.
9. **Desktop transitive dependencies.** Tauri currently pulls unmaintained
   `unic-*` crates through `urlpattern`; monitor upstream replacements. The
   `glib` advisory in the cross-platform lockfile applies to Tauri's Linux
   dependency tree and is not part of the supported macOS console build.
