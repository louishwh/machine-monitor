#!/usr/bin/env bash
# Publish the FleetWatch workspace crates to crates.io in dependency order.
#
#   DRY RUN (default, uploads nothing):   ./scripts/publish-crates.sh
#   REAL PUBLISH:                         ./scripts/publish-crates.sh --execute
#
# Crates are published in topological order: fw-proto first (everything depends
# on it), then the binaries. After fw-proto is published the script waits for
# crates.io to index it before publishing the dependents.
#
# Auth for --execute: run `cargo login` once, or export CARGO_REGISTRY_TOKEN.
#
# Idempotent: a crate whose current version is already on crates.io is skipped,
# so a re-run after a partial failure picks up where it left off.
set -euo pipefail

# Publish order — dependencies BEFORE dependents. Edit this list to control
# exactly what gets published (e.g. comment out binaries to publish only the lib).
PUBLISH_ORDER=(
  fw-proto
  fleetwatch-server
  fleetwatch-agent
  fwctl
  ops-agent
)

EXECUTE=0
ALLOW_DIRTY=0
EXTRA_ARGS=()
for arg in "$@"; do
  case "$arg" in
    --execute)      EXECUTE=1 ;;
    --allow-dirty)  ALLOW_DIRTY=1; EXTRA_ARGS+=(--allow-dirty) ;;
    --no-verify)    EXTRA_ARGS+=(--no-verify) ;;
    -h|--help)
      sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

c_red=$'\033[31m'; c_grn=$'\033[32m'; c_ylw=$'\033[33m'; c_rst=$'\033[0m'
info() { echo "${c_grn}==>${c_rst} $*"; }
warn() { echo "${c_ylw}warn:${c_rst} $*" >&2; }
die()  { echo "${c_red}error:${c_rst} $*" >&2; exit 1; }

command -v cargo >/dev/null || die "cargo not found"
command -v curl  >/dev/null || die "curl not found"

if [ "$ALLOW_DIRTY" -eq 0 ] && [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  die "working tree is dirty — commit/stash first, or pass --allow-dirty"
fi

if [ "$EXECUTE" -eq 1 ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ] && [ ! -f "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" ]; then
  warn "no CARGO_REGISTRY_TOKEN and no ~/.cargo/credentials.toml — run 'cargo login' first if publish fails"
fi

# name -> version, parsed from cargo metadata (no jq dependency).
crate_version() {
  cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c "import sys,json; m=json.load(sys.stdin); print(next((p['version'] for p in m['packages'] if p['name']=='$1'), ''))"
}

# Is <name>@<version> already on crates.io?
already_published() {
  local code
  code=$(curl -fsS -o /dev/null -w '%{http_code}' "https://crates.io/api/v1/crates/$1/$2" 2>/dev/null || echo 000)
  [ "$code" = "200" ]
}

# Block until <name>@<version> is queryable on crates.io (post-publish indexing).
wait_for_index() {
  local name="$1" ver="$2" i
  for i in $(seq 1 30); do
    already_published "$name" "$ver" && { info "$name@$ver is live on crates.io"; return 0; }
    sleep 5
  done
  warn "$name@$ver not visible after 150s — continuing anyway"
}

if [ "$EXECUTE" -eq 1 ]; then
  info "REAL PUBLISH to crates.io"
else
  info "DRY RUN (nothing is uploaded; pass --execute to publish for real)"
fi

for crate in "${PUBLISH_ORDER[@]}"; do
  ver="$(crate_version "$crate")"
  [ -n "$ver" ] || die "could not resolve version for $crate"

  if already_published "$crate" "$ver"; then
    info "skip $crate@$ver (already on crates.io)"
    continue
  fi

  if [ "$EXECUTE" -eq 1 ]; then
    info "publishing $crate@$ver"
    cargo publish -p "$crate" --locked "${EXTRA_ARGS[@]}"
    wait_for_index "$crate" "$ver"
  else
    info "dry-run packaging $crate@$ver"
    # A dependent can't be dry-run-packaged until its workspace deps are on
    # crates.io (cargo resolves the versioned dep against the index). That's
    # expected pre-release — soft-skip it; the real publish orders deps first.
    if out=$(cargo publish -p "$crate" --locked --dry-run "${EXTRA_ARGS[@]}" 2>&1); then
      echo "$out" | tail -3
    elif echo "$out" | grep -q "no matching package named"; then
      warn "$crate: deferred — needs its workspace deps published first (real publish handles ordering)"
    else
      echo "$out" >&2
      die "$crate dry-run failed"
    fi
    # Surface names taken by SOMEONE ELSE (best-effort).
    if curl -fsS "https://crates.io/api/v1/crates/$crate" >/dev/null 2>&1; then
      warn "name '$crate' already exists on crates.io — confirm you own it before --execute"
    fi
  fi
done

if [ "$EXECUTE" -eq 1 ]; then
  info "done — all crates published."
else
  info "dry run OK. Re-run with --execute to publish."
fi
