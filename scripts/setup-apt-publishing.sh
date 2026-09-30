#!/usr/bin/env bash
# One-time signing and Pages setup for this public source repository.
# Run from a machine authenticated with `gh auth login`.
set -euo pipefail
umask 077

command -v gh >/dev/null || { echo "GitHub CLI is required: https://cli.github.com/" >&2; exit 1; }
gh auth status -h github.com >/dev/null 2>&1 || {
    echo "GitHub CLI is not authenticated. Run: gh auth login -h github.com" >&2
    exit 1
}

repo="${FW_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}"
secret_names="$(gh secret list --repo "$repo" --json name --jq '.[].name')"

if grep -Fxq GPG_PRIVATE_KEY <<< "$secret_names"; then
    echo "GPG_PRIVATE_KEY already exists in $repo; leaving the signing key unchanged."
else
    command -v gpg >/dev/null || { echo "GnuPG is required (for example: brew install gnupg)" >&2; exit 1; }
    key_dir="${1:-${XDG_CONFIG_HOME:-$HOME/.config}/fleetwatch/apt-signing}"
    repo_root="$(cd "$(git rev-parse --show-toplevel)" && pwd -P)"
    install -d -m 0700 "$key_dir"
    key_dir="$(cd "$key_dir" && pwd -P)"
    case "$key_dir" in
        "$repo_root"|"$repo_root"/*)
            echo "Refusing to store a private signing key inside the Git repository" >&2
            exit 1
            ;;
    esac
    export GNUPGHOME="$key_dir/gnupg"
    install -d -m 0700 "$GNUPGHOME"

    if ! gpg --batch --list-secret-keys --with-colons | grep -q '^sec:'; then
        gpg --batch --pinentry-mode loopback --passphrase '' \
            --quick-gen-key 'FleetWatch APT repository' ed25519 sign 0
    fi

    fingerprint="$(gpg --batch --list-secret-keys --with-colons | awk -F: '$1 == "fpr" { print $10; exit }')"
    [ -n "$fingerprint" ] || { echo "No signing key found in $GNUPGHOME" >&2; exit 1; }
    gpg --armor --export "$fingerprint" > "$key_dir/public.asc"
    gpg --armor --export-secret-keys "$fingerprint" |
        gh secret set GPG_PRIVATE_KEY --repo "$repo"
    echo "APT signing key uploaded to the GitHub Actions secret GPG_PRIVATE_KEY."
    echo "Public key fingerprint: $fingerprint"
    echo "Keep a secure backup of $GNUPGHOME; GitHub secrets cannot be read back."
fi

pages_endpoint="repos/$repo/pages"
if gh api "$pages_endpoint" --jq .build_type > /dev/null 2>&1; then
    build_type="$(gh api "$pages_endpoint" --jq .build_type)"
    if [ "$build_type" != workflow ]; then
        gh api -X PUT "$pages_endpoint" -f build_type=workflow > /dev/null
    fi
else
    gh api -X POST "$pages_endpoint" -f build_type=workflow > /dev/null
fi

certificate_state="$(gh api "$pages_endpoint" --jq '.https_certificate.state // ""')"
if [ "$certificate_state" = approved ]; then
    gh api -X PUT "$pages_endpoint" -F https_enforced=true > /dev/null
else
    echo "Pages certificate is $certificate_state; enable HTTPS enforcement when it is approved."
fi

environment_endpoint="repos/$repo/environments/github-pages"
if ! gh api "$environment_endpoint" --jq .name > /dev/null 2>&1 ||
   [ "$(gh api "$environment_endpoint" --jq '.deployment_branch_policy.custom_branch_policies')" != true ]; then
    gh api -X PUT "$environment_endpoint" --input - > /dev/null <<'JSON'
{"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}
JSON
fi
policy_endpoint="$environment_endpoint/deployment-branch-policies"
tag_policy="$(gh api "$policy_endpoint" --jq '.branch_policies[] | select(.name == "v*" and .type == "tag") | .id')"
if [ -z "$tag_policy" ]; then
    gh api -X POST "$policy_endpoint" -f name='v*' -f type=tag > /dev/null
fi

echo "Signing and GitHub Pages setup are ready for $repo."
