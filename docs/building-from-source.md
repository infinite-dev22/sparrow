## Building from source

It is easy to build sparrow from source on a GNU/Linux distribution. Cargo handles the build process, all you need to do, is ensure the correct dependencies are installed.

1. Install the Rust compiler and Cargo using [`rustup.rs`](https://rustup.rs/). If you already have the toolchain, use a toolchain that meets the workspace minimum in [Cargo.toml](../Cargo.toml).

2. Install dependencies for your operating system:

#### Ubuntu
```sh
sudo apt install build-essential clang mold libgtk-3-dev libssl-dev libxkbcommon-x11-dev pkg-config libvulkan-dev libwayland-dev xorg-dev libxcb-shape0-dev libxcb-xfixes0-dev
```
#### Fedora
```sh
sudo dnf install gcc mold make clang libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf
```
#### Void Linux
```sh
sudo xbps-install -S base-devel clang mold libxkbcommon-devel vulkan-loader wayland-devel
```

3. Clone this repository (this command will clone to your home directory):
```sh
git clone https://github.com/infinite-dev22/sparrow.git ~/sparrow
```

4. `cd` into the repository, and run the build command with the release flag
```sh
cd ~/sparrow
```

```sh
cargo install --path . --bin sparrow --profile release-lto --locked
```

> If you use a different distribution, and are having trouble finding appropriate dependencies, let us know in an issue!

Once sparrow is compiled, the executable will be available in `$HOME/.cargo/bin/sparrow` and should be available in `PATH` automatically.

## Building using Docker or Podman

For GitHub Releases, see [the release workflow](new-release.md). Distribution packages can be built separately with the existing multi-stage Dockerfiles. To easily orchestrate builds, there is a `docker-bake.hcl` manifest in root of repository that defines all stages and targets.
If you want to build all packages for ubuntu, you can run `RELEASE_TAG_NAME=nightly docker buildx bake ubuntu` (`RELEASE_TAG_NAME` is a required environment variable used to tell what kind of release is being built as well as baking in the version itself).
To scope in to specific distribution version, you can define target with it's version counterpart from matrix, e.g. to build only Ubuntu Focal target, you can run `RELEASE_TAG_NAME=nightly docker buildx bake ubuntu-focal`.
Additionally to building multiple OS versions at the same time, Docker-based builds will also try to cross-compile sparrow for other architectures.
This does not require QEMU installed as it's done via true cross-compilation meaning `HOST` will run your native OS/CPU architecture and `TARGET` will be the wanted architecture, instead of spawning container that's running OS using `TARGET` architecture.

> ![WARNING]
> Do not run plain targets like `ubuntu` or `fedora` if you don't have very powerful machine, as it will spawn many concurrent jobs
> which will take a long time to build.

## Windows and macOS

On Windows, install Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK for your target, plus Perl for the vendored OpenSSL build. On macOS, install the Xcode Command Line Tools with `xcode-select --install`. Then build both binaries from the repository root:

```sh
cargo build --locked --profile release-lto --bins
```

Local builds use `target-cpu=native`. When cross-compiling or sharing a binary, set an explicit CPU target for the destination and retain any required linker or runtime flags. See [CPU optimization](new-release.md#cpu-optimization).
