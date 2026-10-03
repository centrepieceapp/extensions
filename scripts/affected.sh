#!/bin/sh
# Prints the extensions a change touches, one per line.
#
#   scripts/affected.sh <base>    the extensions changed since <base>
#   scripts/affected.sh           every extension
#
# A change to an extension's folder affects that extension. A change to what
# every build shares — the workspace, its lock file (which pins the SDK), the
# toolchain, the build script or the workflow — affects them all. Anything
# else, such as the README, affects none.
set -eu

cd "$(dirname "$0")/.."

all() {
    scripts/extensions.sh
    exit 0
}

base=${1:-}
case "$base" in
    "" | 0000000000000000000000000000000000000000) all ;;
esac
git cat-file -e "$base^{commit}" 2>/dev/null || all

changed=$(git diff --name-only "$base" HEAD)

for file in $changed; do
    case "$file" in
        Cargo.toml | Cargo.lock | rust-toolchain.toml | scripts/extension.sh | .github/workflows/build.yml) all ;;
    esac
done

for name in $(scripts/extensions.sh); do
    if printf '%s\n' "$changed" | grep -q "^$name/"; then
        echo "$name"
    fi
done
