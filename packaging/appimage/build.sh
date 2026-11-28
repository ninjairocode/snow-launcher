#!/bin/bash
# Builds the Snow Launcher AppImage in an Ubuntu 24.04 container (podman),
# so it runs on Ubuntu 24.04 and newer distributions. Run from anywhere:
#     packaging/appimage/build.sh
# Output: target/appimage/Snow_Launcher-<version>-x86_64.AppImage
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
image=snow-launcher-appimage-builder

podman build --quiet --tag "$image" --file "$root/packaging/appimage/Containerfile" \
    "$root/packaging/appimage" > /dev/null
# The crates.io cache lives in a podman volume, kept between builds; the
# build has its own target folder, apart from the host's.
podman run --rm --security-opt label=disable \
    --volume "$root:/src" \
    --volume snow-launcher-cargo-registry:/root/.cargo/registry \
    --env CARGO_TARGET_DIR=/src/target/appimage/cargo \
    --workdir /src \
    "$image" packaging/appimage/assemble.sh
