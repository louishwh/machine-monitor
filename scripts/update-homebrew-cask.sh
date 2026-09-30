#!/usr/bin/env bash
# Render this repository's Homebrew cask from a tagged release DMG.
# Usage: ./scripts/update-homebrew-cask.sh vX.Y.Z path/to/FleetWatch_X.Y.Z_universal.dmg
set -euo pipefail

tag="${1:?version tag required}"
dmg="${2:?DMG path required}"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Expected tag vX.Y.Z" >&2; exit 2; }
version="${tag#v}"
[ -f "$dmg" ] || { echo "DMG not found: $dmg" >&2; exit 1; }
[ "$(basename "$dmg")" = "FleetWatch_${version}_universal.dmg" ] || {
    echo "DMG filename does not match $tag" >&2
    exit 1
}

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
template="$repo_root/packaging/homebrew/Casks/fleetwatch.rb"
output="$repo_root/Casks/fleetwatch.rb"
sha="$(shasum -a 256 "$dmg" | awk '{print $1}')"
url="https://github.com/louishwh/machine-monitor/releases/download/$tag/$(basename "$dmg")"
mkdir -p "$(dirname "$output")"
sed -e "s|@VERSION@|$version|g" -e "s|@SHA256@|$sha|g" \
    -e "s|@URL@|$url|g" "$template" > "$output"
echo "Homebrew cask ready: $output ($version, $sha)"
