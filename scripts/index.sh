#!/bin/sh
# Prints the catalogue Centrepiece lists under "Install extension ▸ From
# official repo": every extension named on the command line, with where its
# archive is published.
#
#   scripts/index.sh centrepieceapp/extensions emoji github > dist/index.json
#
# Each extension's archive is the asset <id>.tar.gz on the release tagged
# <id>-latest, which .github/workflows/build.yml keeps up to date.
set -eu

cd "$(dirname "$0")/.."

[ $# -ge 1 ] || {
    echo "usage: $0 owner/repo [extension...]" >&2
    exit 2
}
repo=$1
shift

# The first `key = "value"` in a file, outside any table.
field() {
    sed -n "/^\[/q; s/^$1 = \"\(.*\)\"/\1/p" "$2" | head -n 1
}

for name in "$@"; do
    manifest=$name/extension.toml
    jq -n \
        --arg id "$(field id "$manifest")" \
        --arg name "$(field name "$manifest")" \
        --arg description "$(field description "$manifest")" \
        --arg prefix "$(field prefix "$manifest")" \
        --arg api "$(field api "$manifest")" \
        --arg version "$(sed -n 's/^version = "\(.*\)"/\1/p' "$name/Cargo.toml" | head -n 1)" \
        --arg url "https://github.com/$repo/releases/download/$name-latest/$name.tar.gz" \
        '{id: $id, name: $name, description: $description, prefix: $prefix, api: $api, version: $version, url: $url}'
done | jq -s '{extensions: .}'
