# Releasing

## crates.io

Workspace crates are published in dependency order by
[`scripts/publish-crates.sh`](scripts/publish-crates.sh):

```
fw-proto            # shared lib — published first
fleetwatch-server   # depends on fw-proto
fleetwatch-agent
fwctl
ops-agent
```

### One-time setup
- `cargo login` (or export `CARGO_REGISTRY_TOKEN`) with a crates.io token that
  owns (or can claim) these crate names.
- Confirm the crate names are available / yours on crates.io. Generic names
  (`fwctl`, `ops-agent`, `fw-proto`) may be taken — the dry run warns you.

### Release steps
1. Bump versions (all crates share `version = "0.1.0"` per crate `Cargo.toml`;
   keep them in sync) and update `CHANGELOG.md`.
2. Commit, tag: `git tag vX.Y.Z && git push --tags`.
3. Dry run (uploads nothing, packages + verifies every crate):
   ```sh
   make publish-dry
   ```
4. Publish for real:
   ```sh
   make publish        # = ./scripts/publish-crates.sh --execute
   ```

### Notes
- The script is **idempotent**: a crate whose current version is already on
  crates.io is skipped, so re-running after a partial failure resumes safely.
- After `fw-proto` is published the script waits for crates.io to index it
  before publishing the dependents.
- To publish only the library, comment out the binaries in `PUBLISH_ORDER` at
  the top of the script.
- Default is **dry run** — nothing uploads without `--execute`.

## Binaries (GitHub Releases)

crates.io is for `cargo install`. For end users who just want a binary, prefer
prebuilt artifacts attached to a GitHub Release:

```sh
make dist     # linux x86_64 server+agent, fwctl, ops-agent, console app
```

(See the `Makefile` for per-target builds, including `build-agent-arm64` /
`build-server-arm64` and the macOS agent.)

## The console app (macOS)

The Tauri console is **not** a crates.io crate. Build it with `make app`
(unsigned by default) or `make app SIGN_IDENTITY="Apple Development: …"` to sign;
notarization is required for distribution outside your own machines.
