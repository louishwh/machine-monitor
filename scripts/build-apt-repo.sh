#!/usr/bin/env bash
# Build a static APT repository layout from a set of .deb files (the layout
# served at https://blog.louishwh.tech/machine-monitor/ from this public
# repository's gh-pages branch, built by the release workflow).
#
#   ./scripts/build-apt-repo.sh <out-dir> <pkg.deb> [more.deb ...]
#
# Produces:
#   <out>/pool/main/binary-<arch>/<pkg>/<file>.deb     (pool split per arch so
#                                                       each Packages index is
#                                                       single-architecture)
#   <out>/dists/stable/main/binary-{amd64,arm64}/Packages{,.gz}
#   <out>/dists/stable/Release{,.gpg} + InRelease
#   <out>/gpg.key                                       (only when signing)
#
# Set FW_GPG_KEY_ID to sign (InRelease + detached Release.gpg + armored
# pubkey). Without it the repo is emitted UNSIGNED — apt only accepts that
# with [trusted=yes], so the release workflow treats a missing key as fatal.
set -euo pipefail

OUT="${1:?usage: build-apt-repo.sh <out-dir> <deb>...}"; shift
[ "$#" -ge 1 ] || { echo "no .deb files given" >&2; exit 1; }

CODENAME="stable"
ARCHS="amd64 arm64"
ORIGIN="FleetWatch"
LABEL="FleetWatch"

command -v apt-ftparchive >/dev/null 2>&1 || { echo "need apt-ftparchive (apt-utils)" >&2; exit 1; }
command -v dpkg-deb >/dev/null 2>&1 || { echo "need dpkg-deb" >&2; exit 1; }

mkdir -p "$OUT"
for deb in "$@"; do
    pkg="$(dpkg-deb -f "$deb" Package)"
    arch="$(dpkg-deb -f "$deb" Architecture)"
    dir="$OUT/pool/main/binary-$arch/$pkg"
    mkdir -p "$dir"
    cp "$deb" "$dir/"
done

for arch in $ARCHS; do
    dir="$OUT/dists/$CODENAME/main/binary-$arch"
    mkdir -p "$dir" "$OUT/pool/main/binary-$arch"
    # Declared arches always get an index, even if no debs of that arch were
    # provided this run — an empty Packages keeps apt aligned with the
    # architectures advertised in the Release file.
    (cd "$OUT" && apt-ftparchive packages "pool/main/binary-$arch" > "dists/$CODENAME/main/binary-$arch/Packages")
    gzip -9n -c "$dir/Packages" > "$dir/Packages.gz"
done

release_args=()
for pair in "Origin=$ORIGIN" "Label=$LABEL" "Suite=$CODENAME" "Codename=$CODENAME" \
            "Architectures=$ARCHS" "Components=main" "Description=FleetWatch packages"; do
    release_args+=("-o" "APT::FTPArchive::Release::${pair%%=*}=${pair#*=}")
done
(cd "$OUT" && apt-ftparchive "${release_args[@]}" release "dists/$CODENAME" > "dists/$CODENAME/Release")

if [ -n "${FW_GPG_KEY_ID:-}" ]; then
    gpg --batch --yes --pinentry-mode loopback --local-user "$FW_GPG_KEY_ID" \
        --output "$OUT/dists/$CODENAME/Release.gpg" \
        --detach-sign "$OUT/dists/$CODENAME/Release"
    gpg --batch --yes --pinentry-mode loopback --local-user "$FW_GPG_KEY_ID" \
        --output "$OUT/dists/$CODENAME/InRelease" \
        --clearsign "$OUT/dists/$CODENAME/Release"
    gpg --armor --export "$FW_GPG_KEY_ID" > "$OUT/gpg.key"
else
    echo "WARNING: FW_GPG_KEY_ID not set — repo is UNSIGNED (apt needs [trusted=yes])" >&2
fi

echo "apt repo ready: $OUT"
