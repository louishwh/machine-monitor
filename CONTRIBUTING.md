# Contributing to FleetWatch

Thanks for your interest in contributing! This guide covers how to build, test,
and submit changes.

## Prerequisites

- **Rust** (stable, recent — 1.80+). Install via [rustup](https://rustup.rs).
- For cross-compiling the Linux server/agent (static musl):
  - [`zig`](https://ziglang.org) (`brew install zig`)
  - `cargo-zigbuild` (`cargo install cargo-zigbuild`)
  - Run `make tools` to add the targets + verify the toolchain.
- For the console (Tauri desktop app):
  - **Node 20+** and **pnpm** (`npm i -g pnpm`).
  - On macOS, the Tauri build also needs the Xcode command-line tools.
- For building the agent `.deb`: `cargo-deb` (`cargo install cargo-deb`), on a
  Debian/Ubuntu host or CI.

## Build, test, lint

Run these from the repo root before opening a PR — CI runs the same checks:

```bash
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Console frontend (separate from the cargo workspace):

```bash
cd console
pnpm install
pnpm build           # tsc + vite (frontend only)
```

## Makefile targets

`make help` lists everything. The most useful:

| Target | What it does |
|--------|--------------|
| `make tools` | One-time: install cross-compile toolchain (zig + targets) |
| `make test`  | `cargo test --workspace` |
| `make build-server` / `make build-agent` | Cross-compile static Linux x86_64 binaries |
| `make build-server-arm64` / `make build-agent-arm64` | Same for aarch64 |
| `make build-fwctl` / `make build-ops-agent` | Host-native operator CLIs |
| `make deb-agent` | Build the agent `.deb` (needs `cargo-deb`, run on Linux/CI) |
| `make app` | Build the console `.app`/`.dmg` (see signing note below) |
| `make app-universal` | Universal (arm64+x86_64) console bundle |
| `make pkg-linux` | Tar up Linux binaries + packaging files |
| `make dist` | Build everything |

(The `deploy-*` targets ssh/scp to your own hosts — they are for maintainers and
not needed to contribute.)

## macOS code signing (console)

`make app` builds the console **unsigned (ad-hoc)** by default, so any
contributor can build it without an Apple certificate. To sign with your own
identity, pass it in — Tauri reads `APPLE_SIGNING_IDENTITY` from the env, which
the Makefile sets from `SIGN_IDENTITY`:

```bash
make app SIGN_IDENTITY="Apple Development: Your Name (TEAMID)"
```

Do **not** commit a personal signing identity into `tauri.conf.json`.

## Branching & pull requests

- Fork the repo (or branch if you have write access). Work on a topic branch
  named like `fix/...` or `feat/...`.
- Keep PRs focused; write a clear description of what and why.
- Make sure `cargo fmt --all --check`, `cargo clippy … -D warnings`,
  `cargo test --workspace`, and `pnpm build` all pass.
- Never commit secrets — `server.toml`, `*.pem`/`*.key`, `.env`, databases, and
  the console master key are gitignored; keep it that way.
- By submitting a contribution you agree to license it under the project's dual
  MIT OR Apache-2.0 license (see [README](README.md#license)).

## Reporting bugs / security issues

- Functional bugs and feature requests: open a GitHub issue (templates provided).
- Security vulnerabilities: **do not** open a public issue — see
  [SECURITY.md](SECURITY.md).
