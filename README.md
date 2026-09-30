# FleetWatch

> Self-hosted fleet monitoring and remote control for your machines — agents
> reverse-connect to one central server, an ed25519 master key is the only key
> to the kingdom. All Rust.

[![CI](https://github.com/louishwh/machine-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/louishwh/machine-monitor/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

FleetWatch lets you watch a fleet of machines and run authorized commands on them
from a single place. You install a lightweight **agent** on each machine; the
agent dials *out* over a TLS WebSocket to a central **server** (so machines need
no inbound ports). You drive everything from a **console** (a Tauri desktop app)
or **`fwctl`** (a headless CLI) — both of which hold an **ed25519 master key**
that is the trust root: the server never issues credentials, it only verifies
signatures made with that key. Every control-plane request is ed25519-signed,
and each agent authenticates with a console-signed identity token that can be
revoked individually.

## Architecture

[查看系统架构网页](https://blog.louishwh.tech/machine-monitor/architecture.html) · [SVG 架构图](docs/architecture.svg)

![FleetWatch architecture](docs/architecture.png)

- **`crates/proto`** — shared protocol: WS messages, ed25519 identity tokens, request signing.
- **`crates/server`** — central server: `/agent` WS hub, `/api/*` signed control plane, SQLite registry/audit, status snapshots, offline scan.
- **`crates/agent`** — single-binary agent: reverse connect, heartbeat with status summary, on-demand collection (sysinfo), controlled shell (off by default, 30s timeout).
- **`crates/fwctl`** — headless console CLI: master key, pairing, identity issuance, list/status/prune.
- **`crates/ops-agent`** — GET-only fleet health observer + alerting; currently uses a copy of the full-control master key, so run it only on a trusted host.
- **`console/`** — Tauri desktop console: master key (file / macOS keychain), pairing, identity issuance, machine list, per-machine terminal, audit, revoke.

## Supported platforms

| Component | Platforms |
|-----------|-----------|
| server    | Linux x86_64 / aarch64 (static musl) |
| agent     | Linux x86_64 / aarch64 (static musl, `.deb`); macOS (launchd) |
| console   | macOS (Tauri app) |
| fwctl / ops-agent | host-native (built for your dev machine) |

## Install

After the one-time publishing setup and first `v*` release, prebuilt `.deb`
packages are published through the signed project APT repository. A new Ubuntu
machine needs the repository configured once; later installs and upgrades use
`apt-get` directly.

**Ubuntu — agent (each monitored machine):**

```bash
curl -fsSLO https://blog.louishwh.tech/machine-monitor/install.sh
sudo sh install.sh agent
```

The script installs the repository public key under `/etc/apt/keyrings`, adds
the signed APT source, runs `apt-get update`, and installs `fleetwatch-agent`.
To configure only the source, run `sudo sh install.sh repo`; then
`sudo apt-get install fleetwatch-agent` works normally.

The service is enabled but not started on install — set `server_url` +
`identity_token` in `/etc/fleetwatch/agent.toml` (from the console's
*issue machine* step or `fwctl issue`), then
`sudo systemctl start fleetwatch-agent`.

**Ubuntu — server (one central host):**

```bash
curl -fsSLO https://blog.louishwh.tech/machine-monitor/install.sh
sudo sh install.sh server
```

Edit `/etc/fleetwatch/server.toml` (set a strong `pairing_token`), then
`sudo systemctl start fleetwatch-server`. The package creates the
`fleetwatch` service user and the TLS directory; the server self-signs a
cert on first boot. See [`packaging/README.md`](packaging/README.md) for the
full deployment ordering.

**macOS — desktop console:**

```bash
brew tap louishwh/fleetwatch https://github.com/louishwh/machine-monitor
brew install --cask louishwh/fleetwatch/fleetwatch
```

Unless the release was signed and notarized (see `RELEASING.md`), macOS
Gatekeeper will ask for confirmation on first launch: right-click → Open.

## Quickstart

Build the workspace and the operator CLI:

```bash
cargo build --workspace
cargo build --release -p fleetwatch-server -p fwctl
```

**1. Configure and run the server.** Copy the example config, set a strong
pairing token, and start:

```bash
cp server.toml.example server.toml
chmod 600 server.toml
# Generate a token with `openssl rand -hex 16`, then paste its output into
# the pairing_token value in server.toml.
# (the server refuses to boot unpaired with a weak/default token)
./target/release/fleetwatch-server      # reads ./server.toml, or set FW_SERVER_CONFIG
```

**2. Pair the console (one-time, TOFU).** With `fwctl`:

```bash
fwctl keygen                                    # create the master key (~/.fleetwatch/master.key)
fwctl pubkey                                     # inspect the public key
fwctl pair --server https://your-server:8443 \
           --pairing-token-prompt \
           --server-ca path/to/cert.pem          # CA needed for the self-signed cert
```

Or open the **console** app, generate a master key, point it at the server, and
pair with the same token.

**3. Issue a machine identity token:**

```bash
fwctl issue --name web-1 --server wss://your-server:8443/agent
# prints the identity token plus the exact `fleetwatch-agent enroll …` command
```

**4. Enroll an agent** on the target machine and start it:

```bash
sudo fleetwatch-agent enroll \
    --server wss://your-server:8443/agent \
    --identity-prompt   # paste the token from step 3 at the hidden prompt
sudo systemctl start fleetwatch-agent     # Linux (.deb); macOS uses launchd
```

**5. Watch and control.** From the console (or `fwctl list` / `fwctl status`)
the machine shows up online; pull live status, open a per-machine terminal
(shell is OFF per machine until you explicitly enable it), review the audit log,
or revoke the identity.

See [`packaging/README.md`](packaging/README.md) for full deployment (systemd,
`.deb`, macOS launchd, console bundle) and the end-to-end ordering, and the
`Makefile` (`make help`) for build/cross-compile/packaging targets.

## Design docs

- Design: `docs/superpowers/specs/2026-05-30-fleetwatch-design.md`
- Plans: `docs/superpowers/plans/` (M1–M5)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, build/test/lint
commands, and conventions, and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
Note: `make app` builds the console **unsigned** by default so anyone can build
it. To sign locally, pass your own identity:

```bash
make app SIGN_IDENTITY="Apple Development: Your Name (TEAMID)"
```

## Security

FleetWatch is a remote-control tool — please read [SECURITY.md](SECURITY.md) for
the security model, known limitations, hardening guidance, and how to report a
vulnerability privately.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

Licensed under either of

- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
