#!/bin/sh
# Builds the extensions in this repository into loadable folders.
#
#   scripts/extension.sh build github     target/extensions/github/, ready for
#                                         `centrepiece --extension target/extensions/github`
#   scripts/extension.sh install github   the same, then copied to
#                                         ~/.config/centrepiece/extensions/github/
#   scripts/extension.sh package github   the same, then archived as
#                                         dist/github.tar.gz, which is what
#                                         Centrepiece's "Install extension" downloads
#   scripts/extension.sh build            every extension
#
# An extension folder is its extension.toml, its assets/, and the component as
# extension.wasm. Centrepiece picks up a rebuilt component the next time the
# extension is entered, so `build` while Centrepiece runs with --extension is
# the edit-run loop.
#
# The SDK is the release Cargo.toml names, from github.com/centrepieceapp/sdk.
# Point CENTREPIECE_SDK at a checkout of it to build against that instead:
#
#   CENTREPIECE_SDK=../sdk scripts/extension.sh build
set -eu

cd "$(dirname "$0")/.."

usage() {
    echo "usage: $0 build|install|package [extension...]" >&2
    exit 2
}

[ $# -ge 1 ] || usage
command=$1
shift
case "$command" in
    build | install | package) ;;
    *) usage ;;
esac

if [ $# -eq 0 ]; then
    set -- $(scripts/extensions.sh)
fi

sdk=
if [ -n "${CENTREPIECE_SDK:-}" ]; then
    sdk_path=$(cd "$CENTREPIECE_SDK" && pwd)
    sdk="patch.'https://github.com/centrepieceapp/sdk'.centrepiece-extension.path='$sdk_path'"
fi

for name in "$@"; do
    source=$name
    [ -f "$source/extension.toml" ] || {
        echo "no extension at $source" >&2
        exit 1
    }
    crate=$(sed -n 's/^name = "\(.*\)"/\1/p' "$source/Cargo.toml" | head -n 1)
    if [ -n "$sdk" ]; then
        # The patch rewrites Cargo.lock to name the local SDK; put it back so
        # the committed lock keeps pinning the one on GitHub.
        mkdir -p target
        cp Cargo.lock target/Cargo.lock.saved
        cargo build --quiet --config "$sdk" -p "$crate" --release --target wasm32-wasip2
        mv target/Cargo.lock.saved Cargo.lock
    else
        cargo build --quiet -p "$crate" --release --target wasm32-wasip2
    fi

    out=target/extensions/$name
    rm -rf "$out.partial"
    mkdir -p "$out.partial"
    cp "$source/extension.toml" "$out.partial/"
    if [ -d "$source/assets" ]; then
        cp -R "$source/assets" "$out.partial/assets"
    fi
    cp "target/wasm32-wasip2/release/$(echo "$crate" | tr - _).wasm" "$out.partial/extension.wasm"
    rm -rf "$out"
    mv "$out.partial" "$out"
    echo "$out"

    case "$command" in
        install)
            dest=${XDG_CONFIG_HOME:-$HOME/.config}/centrepiece/extensions/$name
            mkdir -p "$(dirname "$dest")"
            rm -rf "$dest"
            cp -R "$out" "$dest"
            echo "installed to $dest"
            ;;
        package)
            # One folder, named for the extension, at the top of the archive.
            mkdir -p dist
            # COPYFILE_DISABLE keeps macOS tar from adding ._ files.
            COPYFILE_DISABLE=1 tar -czf "dist/$name.tar.gz" -C target/extensions "$name"
            echo "dist/$name.tar.gz"
            ;;
    esac
done
