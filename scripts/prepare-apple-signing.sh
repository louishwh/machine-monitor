#!/usr/bin/env bash
# Import release credentials into an ephemeral GitHub Actions keychain.
# Secret values arrive through environment variables, never command-line inputs.
set -euo pipefail
umask 077

[ "${GITHUB_ACTIONS:-}" = true ] || {
    echo "This script only runs on an ephemeral GitHub Actions runner." >&2
    exit 1
}
: "${RUNNER_TEMP:?}" "${GITHUB_ENV:?}" "${GITHUB_OUTPUT:?}"

names=(FW_APPLE_CERTIFICATE FW_APPLE_CERTIFICATE_PASSWORD FW_APPLE_SIGNING_IDENTITY
       FW_APPLE_API_ISSUER FW_APPLE_API_KEY_ID FW_APPLE_API_KEY)
present=0
for name in "${names[@]}"; do
    if [ -n "${!name:-}" ]; then present=$((present + 1)); fi
done
if [ "$present" -eq 0 ]; then
    echo 'enabled=false' >> "$GITHUB_OUTPUT"
    echo "Apple release credentials are absent; building with ad-hoc signing."
    exit 0
fi
for name in "${names[@]}"; do
    [ -n "${!name:-}" ] || { echo "Missing release secret: ${name#FW_}" >&2; exit 1; }
done
case "$FW_APPLE_SIGNING_IDENTITY" in
    "Developer ID Application: "*) ;;
    *) echo "Public releases require a Developer ID Application identity." >&2; exit 1 ;;
esac

signing_dir="$RUNNER_TEMP/fleetwatch-apple-signing"
keychain="$signing_dir/signing.keychain-db"
install -d -m 0700 "$signing_dir"
printf '%s' "$FW_APPLE_CERTIFICATE" | base64 --decode > "$signing_dir/certificate.p12"
printf '%s\n' "$FW_APPLE_API_KEY" > "$signing_dir/AuthKey.p8"
grep -q '^-----BEGIN PRIVATE KEY-----$' "$signing_dir/AuthKey.p8" || {
    echo "APPLE_API_KEY must contain the downloaded App Store Connect .p8 key." >&2
    exit 1
}

keychain_password="$(openssl rand -hex 32)"
printf '::add-mask::%s\n' "$keychain_password"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$signing_dir/certificate.p12" -k "$keychain" \
    -P "$FW_APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign > /dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s \
    -k "$keychain_password" "$keychain" > /dev/null
security list-keychains -d user -s "$keychain" "$HOME/Library/Keychains/login.keychain-db"
security find-identity -v -p codesigning "$keychain" |
    awk -F '"' -v wanted="$FW_APPLE_SIGNING_IDENTITY" \
        '$2 == wanted { found = 1 } END { exit !found }' || {
    echo "The imported certificate does not match APPLE_SIGNING_IDENTITY." >&2
    exit 1
}
rm -f "$signing_dir/certificate.p12"
printf 'APPLE_API_KEY_PATH=%s\n' "$signing_dir/AuthKey.p8" >> "$GITHUB_ENV"
echo 'enabled=true' >> "$GITHUB_OUTPUT"
echo "Developer ID certificate imported into the temporary release keychain."
