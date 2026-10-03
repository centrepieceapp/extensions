#!/bin/sh
# Lists every extension in this repository: each top-level folder with an
# extension.toml. Its folder name is its id, which the workflow relies on.
set -eu

cd "$(dirname "$0")/.."

for manifest in */extension.toml; do
    dirname "$manifest"
done
