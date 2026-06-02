#!/usr/bin/env bash
# Install the FleetWatch agent as a macOS LaunchDaemon (system-level, runs at boot).
# Usage:
#   sudo ./install-macos.sh <path-to-fleetwatch-agent-binary> <server_url> <identity_token>
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "Please run with sudo (LaunchDaemon installs system-wide)." >&2
    exit 1
fi
if [ "$#" -ne 3 ]; then
    echo "Usage: sudo $0 <agent-binary> <server_url> <identity_token>" >&2
    exit 1
fi

BIN="$1"; SERVER_URL="$2"; TOKEN="$3"
PLIST_SRC="$(cd "$(dirname "$0")" && pwd)/com.fleetwatch.agent.plist"
PLIST_DST="/Library/LaunchDaemons/com.fleetwatch.agent.plist"

install -m 755 "$BIN" /usr/local/bin/fleetwatch-agent
mkdir -p /etc/fleetwatch
cat > /etc/fleetwatch/agent.toml <<EOF
server_url = "${SERVER_URL}"
identity_token = "${TOKEN}"
EOF
chmod 600 /etc/fleetwatch/agent.toml

cp "$PLIST_SRC" "$PLIST_DST"
chown root:wheel "$PLIST_DST"
chmod 644 "$PLIST_DST"

launchctl unload "$PLIST_DST" 2>/dev/null || true
launchctl load "$PLIST_DST"

echo "FleetWatch agent installed and loaded (com.fleetwatch.agent)."
echo "Logs: /var/log/fleetwatch-agent.log"
