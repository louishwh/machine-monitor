#!/bin/sh
# Configure the signed FleetWatch APT repository on Ubuntu and optionally
# install fleetwatch-agent or fleetwatch-server.
#
# Usage: sudo sh install.sh [agent|server|repo]
set -eu

REPO_URL="https://blog.louishwh.tech/machine-monitor"
KEYRING="/etc/apt/keyrings/fleetwatch.asc"
SOURCE="/etc/apt/sources.list.d/fleetwatch.list"

case "${1:-}" in
    agent) package="fleetwatch-agent" ;;
    server) package="fleetwatch-server" ;;
    repo) package="" ;;
    *) echo "Usage: sudo sh install.sh agent|server|repo" >&2; exit 2 ;;
esac
[ "$#" -eq 1 ] || { echo "Expected exactly one argument" >&2; exit 2; }
[ "$(id -u)" -eq 0 ] || { echo "Run this script with sudo" >&2; exit 1; }
command -v apt-get >/dev/null 2>&1 || { echo "apt-get is required" >&2; exit 1; }
command -v dpkg >/dev/null 2>&1 || { echo "dpkg is required" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "curl is required (sudo apt-get install curl)" >&2; exit 1; }

arch="$(dpkg --print-architecture)"
case "$arch" in
    amd64|arm64) ;;
    *) echo "Unsupported architecture: $arch (available: amd64, arm64)" >&2; exit 1 ;;
esac

temp_key="$(mktemp)"
trap 'rm -f "$temp_key"' EXIT HUP INT TERM
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    "$REPO_URL/gpg.key" --output "$temp_key"
if ! grep -q '^-----BEGIN PGP PUBLIC KEY BLOCK-----$' "$temp_key" ||
   ! grep -q '^-----END PGP PUBLIC KEY BLOCK-----$' "$temp_key"; then
    echo "The repository signing key could not be read from $REPO_URL/gpg.key" >&2
    exit 1
fi

install -d -m 0755 /etc/apt/keyrings
install -m 0644 "$temp_key" "$KEYRING"
printf 'deb [arch=%s signed-by=%s] %s stable main\n' "$arch" "$KEYRING" "$REPO_URL" > "$SOURCE"

apt-get update
if [ -n "$package" ]; then
    apt-get install -y "$package"
fi

echo "FleetWatch APT repository is ready."
if [ "$package" = "fleetwatch-agent" ]; then
    echo "Next: enroll this machine with a console-issued identity token, then start fleetwatch-agent."
elif [ "$package" = "fleetwatch-server" ]; then
    echo "Next: set pairing_token in /etc/fleetwatch/server.toml, then start fleetwatch-server."
else
    echo "Install packages with: apt-get install fleetwatch-agent  (or fleetwatch-server)"
fi
