#!/bin/sh
# Rebuild vendor/liquid-core from the pristine crates.io tarball plus our patch.
# The patch (vendor/rekyll-liquid-core.patch) is the source of truth; the copy
# is checked in so builds work offline.
set -eu
cd "$(dirname "$0")/.."
VERSION=0.26.11
SRC=$(ls -d ~/.cargo/registry/src/*/liquid-core-$VERSION | head -n1)
rm -rf vendor/liquid-core
cp -r "$SRC" vendor/liquid-core
rm -f vendor/liquid-core/Cargo.lock vendor/liquid-core/Cargo.toml.orig vendor/liquid-core/.cargo_vcs_info.json vendor/liquid-core/.cargo-ok
patch -p1 -d vendor/liquid-core < vendor/rekyll-liquid-core.patch
