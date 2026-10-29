#!/usr/bin/env bash
# build-rpm.sh - build the sparrow RPM natively on Fedora, no Docker.
# Mirrors extra/linux/docker/fedora/Dockerfile. Run from anywhere inside the repo.
#
#   ./build-rpm.sh                            # local build -> version <crate>+<timestamp>
#   RELEASE_TAG_NAME=v0.4.6 ./build-rpm.sh    # release-style version 0.4.6
#   SKIP_DEPS=1 ./build-rpm.sh                # skip the dnf step
#
# Output: target/rpm/*.rpm
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PACKAGE_NAME="${PACKAGE_NAME:-sparrow}"
RELEASE_TAG_NAME="${RELEASE_TAG_NAME:-local}"

# --- 1. System dependencies -------------------------------------------------
# Package list = RHEL_FAMILY_PACKAGES from upstream docker-bake.hcl, plus the
# toolchain pieces the container image provides (clang, lld, cmake, rpm-build...).
if [[ "${SKIP_DEPS:-0}" != "1" ]]; then
  sudo dnf install -y \
    bash clang lld llvm file cmake pkgconf-pkg-config curl git rpm-build \
    gcc gcc-c++ make \
    openssl-devel wayland-devel vulkan-loader-devel libzstd-devel \
    libxcb-devel libxkbcommon-x11-devel
fi

command -v cargo >/dev/null || { echo "cargo not found - install rustup first" >&2; exit 1; }

# --- 2. Sanity-check files the spec installs ---------------------------------
for f in \
  extra/linux/dev.sparrow.sparrow.desktop \
  extra/linux/dev.sparrow.sparrow.metainfo.xml \
  extra/images/logo.png; do
  [[ -f "$f" ]] || { echo "missing $f (referenced by the spec)" >&2; exit 1; }
done

# --- 3. Version --------------------------------------------------------------
CRATE_VERSION="$(cargo pkgid | cut -d'#' -f2 | cut -d'@' -f2 | cut -d':' -f2)"
case "$RELEASE_TAG_NAME" in
  nightly-*)            VERSION="${CRATE_VERSION}+${RELEASE_TAG_NAME#nightly-}" ;;
  local|debug|nightly)  VERSION="${CRATE_VERSION}+$(date +%Y%m%d%H%M)" ;;
  *)                    VERSION="${RELEASE_TAG_NAME#v}" ;;
esac
echo ">> building ${PACKAGE_NAME} ${VERSION}"

# --- 4. Build environment (same as the Dockerfile) ---------------------------
export RELEASE_TAG_NAME="$VERSION"
export OPENSSL_NO_VENDOR=1            # link system openssl-devel, no Perl build
export ZSTD_SYS_USE_PKG_CONFIG=1
export CC=clang CXX=clang++
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export RUSTFLAGS="${RUSTFLAGS:--C linker=clang -C link-arg=-fuse-ld=mold}"

cargo fetch --locked

# --- 5. Spec -----------------------------------------------------------------
SPEC="$(mktemp --suffix=.spec)"
trap 'rm -f "$SPEC"' EXIT

cat > "$SPEC" <<'EOL'
Name:     %{pkg_name}
Version:  %{pkg_version}
Release:  1%{?dist}
Summary:  Lightning-fast and Powerful Code Editor written in Rust
License:  Apache-2.0
URL:      https://github.com/infinite-dev22/sparrow

%description
sparrow is written in pure Rust with a UI in Floem (which is also written in Rust).
It is designed with Rope Science from the Xi-Editor which makes for lightning-fast computation, and leverages OpenGL for rendering.

%build
cargo build --profile release-lto --bin sparrow --locked

%install
install -Dm755 target/release-lto/sparrow %{buildroot}%{_bindir}/sparrow
install -Dm644 extra/linux/dev.sparrow.sparrow.desktop %{buildroot}/usr/share/applications/dev.sparrow.sparrow.desktop
install -Dm644 extra/linux/dev.sparrow.sparrow.metainfo.xml %{buildroot}/usr/share/metainfo/dev.sparrow.sparrow.metainfo.xml
install -Dm644 extra/images/logo.png %{buildroot}/usr/share/pixmaps/dev.sparrow.sparrow.png

%files
%license LICENSE*
%doc *.md
%{_bindir}/sparrow
/usr/share/applications/dev.sparrow.sparrow.desktop
/usr/share/metainfo/dev.sparrow.sparrow.metainfo.xml
/usr/share/pixmaps/dev.sparrow.sparrow.png

EOL
printf '%%changelog\n* %s %s\n- Local build\n' \
  "$(LC_ALL=C date '+%a %b %d %Y')" "$(git config user.name || echo builder)" >> "$SPEC"

# --- 6. Build the RPM in place -----------------------------------------------
# --noclean matters: with --build-in-place the build dir IS this repo checkout.
rpmbuild --build-in-place --noprep --noclean \
  -D 'debug_package %{nil}' \
  -D "pkg_name ${PACKAGE_NAME}" \
  -D "pkg_version ${VERSION}" \
  -bb "$SPEC"

# --- 7. Collect output -------------------------------------------------------
mkdir -p target/rpm
find "$(rpm --eval '%{_rpmdir}')" -name "${PACKAGE_NAME}-${VERSION}-*.rpm" -exec cp -v {} target/rpm/ \;

echo
echo "Done. Install with:  sudo dnf install ./target/rpm/${PACKAGE_NAME}-*.rpm"