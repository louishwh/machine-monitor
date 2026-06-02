#!/usr/bin/env bash
# Build the FleetWatch agent .deb package.
# Run on a Debian/Ubuntu host (or a Linux CI runner) with the Rust toolchain.
#
#   cargo install cargo-deb         # once
#   ./scripts/build-deb.sh
#
# Output: target/debian/fleetwatch-agent_<version>_<arch>.deb
# Install: sudo apt install ./fleetwatch-agent_<version>_<arch>.deb
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo-deb >/dev/null 2>&1; then
    echo "cargo-deb not found. Install with: cargo install cargo-deb" >&2
    exit 1
fi

cargo build --release -p fleetwatch-agent
cargo deb -p fleetwatch-agent --no-build

echo "Done. .deb is under target/debian/"
