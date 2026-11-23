#!/usr/bin/env bash
# Run inside the Debian container for the release architecture.
set -euo pipefail

apt-get update
apt-get install -y --no-install-recommends \
    build-essential ca-certificates clang curl git libegl1-mesa-dev libgtk-3-dev \
    libssl-dev libvulkan-dev libwayland-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-x11-dev pkg-config xorg-dev

# Checkout belongs to the runner account. Cargo's build metadata reads Git.
git config --global --add safe.directory /src
curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | \
    sh -s -- -y --profile minimal --default-host "$RELEASE_TARGET" --default-toolchain stable
export PATH="$HOME/.cargo/bin:$PATH"

# An explicit RUSTFLAGS replaces the local Cargo flags for this build.
export RUSTFLAGS="-C target-cpu=$RELEASE_CPU"
cargo build --locked --profile release-lto --target "$RELEASE_TARGET" --bins

bin="target/$RELEASE_TARGET/release-lto"
"$bin/sparrow" --version
"$bin/sparrow-proxy" --version
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/Sparrow" dist
install -m755 "$bin/sparrow" "$stage/Sparrow/sparrow"
install -m755 "$bin/sparrow-proxy" "$stage/Sparrow/sparrow-proxy"
tar -czf "dist/$RELEASE_ASSET" -C "$stage" Sparrow
gzip -c "$bin/sparrow-proxy" > "dist/sparrow-proxy-linux-$RELEASE_ARCH.gz"
