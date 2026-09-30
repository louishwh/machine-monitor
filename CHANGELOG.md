# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-09-30

### Added
- Central **server** (Axum) with reverse-connect agent WebSocket, self-signed
  TLS (rcgen), and a signature-authenticated control plane.
- **Agent** (`fleetwatch-agent`): reverse-WS client, status collectors
  (host/cpu/mem/disk/net/proc/service), heartbeat with hardware-spec summary,
  and gated remote shell. Ships for Linux (x86_64/aarch64, systemd) and
  macOS (launchd).
- **Console** (Tauri + React): multi-server profiles with a switcher and a
  server-settings dialog, machine list with live CPU/mem/disk and aligned
  columns, machine detail (status/snapshots/audit/shell/revoke), and a
  per-machine REPL-style terminal.
- **fwctl** headless console CLI: `keygen`, `pubkey`, `pair`, `issue`
  (with `--machine-id` to reuse an id / rename in place), `list`, `status`,
  `prune`.
- Server endpoints: `DELETE /api/machines/:id` (delete-machine) and the
  `fwctl prune` flow to clean stale/offline records.
- ed25519 trust model: console master key is the trust root; one-time pairing
  registers the console public key; control-plane requests are signed
  (timestamp window ±300s); agents authenticate with console-signed identity
  tokens.

### Fixed
- **agent**: timed-out shell commands are now killed and reaped. Tokio does
  not kill a dropped `Child` by default, so every timed-out command used to
  survive as an orphan process — a long-running agent accumulated one leaked
  process per timed-out command (unbounded memory and CPU growth).
- **agent**: reconnect backoff no longer resets on a fresh connection that the
  server immediately rejects. A revoked/invalid agent used to reconnect with a
  full TLS handshake about once per second forever; explicit rejections now
  back off 5 minutes, and only sessions that stayed up ≥ 60 s reset the
  backoff to 1 s.
- **agent**: command execution no longer blocks the agent's session loop — a
  30 s shell used to stall heartbeats and delay revocation detection. Results
  are now sent from independent tasks over the shared write half.
- **server**: the six end-to-end flow tests (`pair`/`auth`/`status`/`shell`/
  `tls`/`online`) were never compiled — they lived under `src/tests/` with no
  `mod` declaration. Moved to the conventional `crates/server/tests/`
  integration-test location so `cargo test` (and CI) actually runs them.

### Security
- Pairing hardening: the server refuses to boot while unpaired if
  `pairing_token` is the default/weak value (< 16 chars); the token comparison
  is constant-time; failed `/api/pair` attempts are delayed and logged.

[Unreleased]: https://github.com/louishwh/machine-monitor/commits/main
