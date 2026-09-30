#!/usr/bin/env bash
# Update the Homebrew cask in the louishwh/homebrew-tap repo after a release.
#
#   ./scripts/publish-tap.sh v0.1.0
#
# Env:
#   FW_TAP_TOKEN    (required) PAT with contents:write on the tap repo
#   FW_TAP_REPO     (default louishwh/homebrew-tap)
#   FW_SOURCE_REPO  (default louishwh/machine-monitor)
#
# Finds the .dmg asset on the GitHub release, computes its sha256, fills in
# packaging/homebrew/Casks/fleetwatch.rb and pushes it to the tap.
set -euo pipefail

TAG="${1:?usage: publish-tap.sh <tag>}"
VER="${TAG#v}"
TAP_REPO="${FW_TAP_REPO:-louishwh/homebrew-tap}"
SRC_REPO="${FW_SOURCE_REPO:-louishwh/machine-monitor}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEMPLATE="$SCRIPT_DIR/../packaging/homebrew/Casks/fleetwatch.rb"

[ -n "${FW_TAP_TOKEN:-}" ] || { echo "FW_TAP_TOKEN is required" >&2; exit 1; }
[ -f "$TEMPLATE" ] || { echo "cask template not found: $TEMPLATE" >&2; exit 1; }
command -v gh >/dev/null 2>&1 || { echo "gh CLI required" >&2; exit 1; }
export GH_TOKEN="$FW_TAP_TOKEN"

asset="$(gh release view "$TAG" --repo "$SRC_REPO" --json assets --jq '.assets[].name' \
    | grep '\.dmg$' | head -n1)"
[ -n "$asset" ] || { echo "no .dmg asset found on release $TAG of $SRC_REPO" >&2; exit 1; }
url="https://github.com/$SRC_REPO/releases/download/$TAG/$asset"
echo "asset: $asset"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$url" -o "$tmp/app.dmg"
sha="$(shasum -a 256 "$tmp/app.dmg" | cut -d' ' -f1)"

gh auth setup-git --hostname github.com
git clone --depth 1 "https://github.com/${TAP_REPO}.git" "$tmp/tap"
cask="$tmp/tap/Casks/fleetwatch.rb"
mkdir -p "$(dirname "$cask")"
sed -e "s|@VERSION@|$VER|g" -e "s|@SHA256@|$sha|g" -e "s|@URL@|$url|g" "$TEMPLATE" > "$cask"

cd "$tmp/tap"
git config user.name "fleetwatch-release-bot"
git config user.email "actions@github.com"
git add -A
git commit -m "fleetwatch $VER"
git push origin HEAD
echo "cask updated: $TAP_REPO Casks/fleetwatch.rb -> $VER ($sha)"
