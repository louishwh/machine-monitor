#!/bin/sh
# Signed APT installation, enrollment and automatic service startup on Ubuntu.
# Run: curl -fsSL https://blog.louishwh.tech/machine-monitor/install.sh | sudo sh -s -- agent
set -eu

usage() {
    cat <<'EOF'
Usage: install.sh agent [--server URL] [--identity-file PATH] [--server-ca PATH]
                        [--server-ca-base64 PUBLIC_CERT] [--allow-insecure] [--install-only]
       install.sh server|repo

Agent mode installs, checks enrollment, saves a private configuration, and
starts a service enabled on boot. Missing server and identity are requested
from the controlling terminal. Use --identity-file for unattended deployment.
--server-ca pins a self-signed server certificate; TLS verification stays enabled.
--install-only installs the package without enrollment or service startup.
EOF
}

case "${1:-}" in
    agent) package="fleetwatch-agent" ;;
    server) package="fleetwatch-server" ;;
    repo) package="" ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac
mode="$1"; shift
server=""; identity_file=""; server_ca=""; server_ca_base64=""; install_only=false; allow_insecure=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        --server|--identity-file|--server-ca|--server-ca-base64)
            [ "$#" -ge 2 ] && [ -n "$2" ] || { echo "Missing value for $1" >&2; exit 2; }
            case "$1" in
                --server) server="$2" ;;
                --identity-file) identity_file="$2" ;;
                --server-ca) server_ca="$2" ;;
                --server-ca-base64) server_ca_base64="$2" ;;
            esac
            shift 2 ;;
        --install-only) install_only=true; shift ;;
        --allow-insecure) allow_insecure=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; exit 2 ;;
    esac
done
if [ "$mode" != agent ] && { [ -n "$server$identity_file$server_ca$server_ca_base64" ] || [ "$install_only" = true ] || [ "$allow_insecure" = true ]; }; then
    echo "Enrollment options are only supported in agent mode." >&2
    exit 2
fi
if [ "$install_only" = true ] && { [ -n "$server$identity_file$server_ca$server_ca_base64" ] || [ "$allow_insecure" = true ]; }; then
    echo "--install-only cannot be combined with enrollment options." >&2
    exit 2
fi
if [ -n "$server_ca" ] && [ -n "$server_ca_base64" ]; then
    echo "Use either --server-ca or --server-ca-base64, not both." >&2
    exit 2
fi

[ "$(id -u)" -eq 0 ] || { echo "Run this script with sudo" >&2; exit 1; }
for tool in apt-get dpkg curl; do
    command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required" >&2; exit 1; }
done
arch="$(dpkg --print-architecture)"
case "$arch" in
    amd64|arm64) ;;
    *) echo "Unsupported architecture: $arch (available: amd64, arm64)" >&2; exit 1 ;;
esac
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
if [ -n "$server_ca_base64" ]; then
    server_ca="$work_dir/server-ca.pem"
    printf '%s' "$server_ca_base64" | base64 --decode > "$server_ca" || {
        echo "Invalid public certificate encoding." >&2
        exit 2
    }
fi

if [ "$mode" = agent ] && [ "$install_only" != true ]; then
    if ! command -v systemctl >/dev/null 2>&1 || ! systemctl show --property=Version --value >/dev/null 2>&1; then
        echo "Agent auto-start requires a running systemd manager. Use --install-only for a container." >&2
        exit 1
    fi
    if [ -z "$server" ] || [ -z "$identity_file" ]; then
        if ! (: </dev/tty) 2>/dev/null; then
            echo "No interactive terminal. Provide --server and --identity-file for unattended installation." >&2
            exit 1
        fi
    fi
    if [ -z "$server" ]; then
        printf 'FleetWatch server URL (https://host or wss://host/agent): ' > /dev/tty
        IFS= read -r server < /dev/tty
    fi
    case "$server" in
        *'?'*|*'#'*|*'@'*|*'"'*|*"'"*|*[[:space:]]*) echo "Server URL must not contain credentials, whitespace, queries or fragments." >&2; exit 2 ;;
    esac
    case "$server" in
        https://*) server="wss://${server#https://}" ;;
        wss://*) ;;
        http://*|ws://*)
            [ "$allow_insecure" = true ] || { echo "Use HTTPS/WSS, or explicitly pass --allow-insecure for a trusted local network." >&2; exit 2; }
            case "$server" in http://*) server="ws://${server#http://}" ;; esac ;;
        *) echo "Server URL must use https:// or wss://" >&2; exit 2 ;;
    esac
    case "${server#*://}" in
        ""|/*) echo "Server URL needs a hostname." >&2; exit 2 ;;
    esac
    server="${server%/}"
    case "$server" in */agent) ;; *) server="$server/agent" ;; esac
    if [ -n "$identity_file" ]; then
        [ -f "$identity_file" ] && [ -r "$identity_file" ] || { echo "Identity file is not readable: $identity_file" >&2; exit 1; }
    fi
    if [ -n "$server_ca" ]; then
        [ -f "$server_ca" ] && [ -r "$server_ca" ] || { echo "Server CA file is not readable: $server_ca" >&2; exit 1; }
    fi
fi

REPO_URL="https://blog.louishwh.tech/machine-monitor"
KEYRING="/etc/apt/keyrings/fleetwatch.asc"
SOURCE="/etc/apt/sources.list.d/fleetwatch.list"
temp_key="$work_dir/gpg.key"
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --connect-timeout 15 --max-time 60 "$REPO_URL/gpg.key" --output "$temp_key"
if ! grep -q '^-----BEGIN PGP PUBLIC KEY BLOCK-----$' "$temp_key" ||
   ! grep -q '^-----END PGP PUBLIC KEY BLOCK-----$' "$temp_key"; then
    echo "The repository signing key could not be read from $REPO_URL/gpg.key" >&2
    exit 1
fi
install -d -m 0755 /etc/apt/keyrings
install -m 0644 "$temp_key" "$KEYRING"
printf 'deb [arch=%s signed-by=%s] %s stable main\n' "$arch" "$KEYRING" "$REPO_URL" > "$SOURCE"

# Subcommands must not consume the install script when it is piped to sh.
apt-get update < /dev/null
if [ -n "$package" ]; then
    DEBIAN_FRONTEND=noninteractive apt-get install -y -o Dpkg::Options::=--force-confold "$package" < /dev/null
fi

if [ "$mode" = agent ] && [ "$install_only" != true ]; then
    fleetwatch-agent enroll --help | grep -q -- '--check' || {
        echo "Agent package is too old for automatic enrollment; version 0.1.2 or later is required." >&2
        exit 1
    }
    set -- fleetwatch-agent enroll --server "$server" --check
    if [ -n "$identity_file" ]; then set -- "$@" --identity-file "$identity_file";
    else set -- "$@" --identity-prompt; fi
    if [ -n "$server_ca" ]; then set -- "$@" --server-ca "$server_ca"; fi
    "$@" < /dev/null
    systemctl daemon-reload
    systemctl enable fleetwatch-agent.service
    systemctl restart fleetwatch-agent.service
    systemctl is-active --quiet fleetwatch-agent.service || {
        echo "Agent could not start. Inspect: journalctl -u fleetwatch-agent -n 30" >&2
        exit 1
    }
    echo "FleetWatch Agent is running and enabled on boot. Check the console for this machine."
elif [ "$mode" = agent ]; then
    echo "Agent package installed. Enroll this machine before starting the service."
elif [ "$mode" = server ]; then
    echo "Server package installed. Set pairing_token in /etc/fleetwatch/server.toml, then start fleetwatch-server."
else
    echo "FleetWatch APT repository is ready. Install with apt-get install fleetwatch-agent or fleetwatch-server."
fi
