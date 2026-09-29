#!/usr/bin/env bash
# Build the FleetWatch .deb packages (agent + server).
#
#   ./scripts/build-deb.sh                                # host glibc build (Linux)
#   ./scripts/build-deb.sh x86_64-unknown-linux-musl      # static musl cross-build
#   ./scripts/build-deb.sh aarch64-unknown-linux-musl
#
# Cross-builds need zig + cargo-zigbuild + the rust target (one-time: `make tools`).
# Output: target/[<triple>/]debian/fleetwatch-{agent,server}_<version>_<arch>.deb
# Install: sudo apt install ./fleetwatch-agent_*.deb ./fleetwatch-server_*.deb
set -euo pipefail
cd "$(dirname "$0")/.."

PKGS=(fleetwatch-agent fleetwatch-server)
TARGET="${1:-}"

if ! command -v cargo-deb >/dev/null 2>&1; then
    echo "cargo-deb not found. Install with: cargo install cargo-deb" >&2
    exit 1
fi

if [ -n "$TARGET" ]; then
    if ! command -v cargo-zigbuild >/dev/null 2>&1; then
        echo "cargo-zigbuild not found. One-time setup: make tools" >&2
        exit 1
    fi
    cargo zigbuild --release --target "$TARGET" -p fleetwatch-agent -p fleetwatch-server
    DEB_FLAGS=(--target "$TARGET" --no-build)
else
    cargo build --release -p fleetwatch-agent -p fleetwatch-server
    DEB_FLAGS=(--no-build)
fi

for pkg in "${PKGS[@]}"; do
    cargo deb -p "$pkg" "${DEB_FLAGS[@]}"
done

echo "Done. .debs are under target/[<triple>/]debian/"
