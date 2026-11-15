#!/usr/bin/env bash
# build-rpm.sh - build the sparrow RPM natively on Fedora, no Docker.
# Mirrors extra/linux/docker/fedora/Dockerfile. Run from anywhere inside the repo.
#
#   ./build-rpm.sh                            # local build -> version <crate>+<timestamp>
#   RELEASE_TAG_NAME=v0.4.6 ./build-rpm.sh    # release-style version 0.4.6
#   SKIP_DEPS=1 ./build-rpm.sh                # skip the dnf step
#   SKIP_BUILD=1 ./build-rpm.sh               # don't run cargo; package the existing
#                                             #   target/release-lto/sparrow as-is
#   NO_CLEAN=1 ./build-rpm.sh                 # keep older RPMs in target/rpm
#
# Every run stamps a unique RPM Release (1.<timestamp>), so dnf always treats a
# rebuild as an upgrade, even when the version (e.g. a release tag) is unchanged.
#
# Output: target/rpm/*.rpm  (only the RPM from this run)
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

PACKAGE_NAME="${PACKAGE_NAME:-sparrow}"
RELEASE_TAG_NAME="${RELEASE_TAG_NAME:-local}"

# --- 1. System dependencies -------------------------------------------------
# Package list = RHEL_FAMILY_PACKAGES from upstream docker-bake.hcl, plus the
# toolchain pieces the container image provides (clang, lld, mold, cmake, rpm-build...).
if [[ "${SKIP_DEPS:-0}" != "1" ]]; then
  sudo dnf install -y \
    bash clang lld mold llvm file cmake pkgconf-pkg-config curl git rpm-build \
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

SKIP_BUILD="${SKIP_BUILD:-0}"
NO_CLEAN="${NO_CLEAN:-0}"
BIN="target/release-lto/sparrow"

if [[ "$SKIP_BUILD" == "1" ]]; then
  [[ -x "$BIN" ]] || { echo "SKIP_BUILD=1 but $BIN does not exist - build it first" >&2; exit 1; }
  echo ">> SKIP_BUILD=1: packaging existing $BIN (last modified: $(date -r "$BIN" '+%F %T'))"
fi

# --- 3. Version --------------------------------------------------------------
CRATE_VERSION="$(cargo pkgid | cut -d'#' -f2 | cut -d'@' -f2 | cut -d':' -f2)"
case "$RELEASE_TAG_NAME" in
  nightly-*)            VERSION="${CRATE_VERSION}+${RELEASE_TAG_NAME#nightly-}" ;;
  local|debug|nightly)  VERSION="${CRATE_VERSION}+$(date +%Y%m%d%H%M)" ;;
  *)                    VERSION="${RELEASE_TAG_NAME#v}" ;;
esac
# Unique per run so a rebuild of the same version still upgrades via dnf.
RELEASE_STAMP="1.$(date +%Y%m%d%H%M%S)"
echo ">> building ${PACKAGE_NAME} ${VERSION}-${RELEASE_STAMP}"

# --- 4. Build environment (same as the Dockerfile) ---------------------------
export RELEASE_TAG_NAME="$VERSION"
export OPENSSL_NO_VENDOR=1            # link system openssl-devel, no Perl build
export ZSTD_SYS_USE_PKG_CONFIG=1
export CC=clang CXX=clang++
export CARGO_NET_GIT_FETCH_WITH_CLI=true
# NOTE: these flags differ from a plain `cargo build`, so cargo keeps a separate
# fingerprint for them and will rebuild. To make a manual build identical to the
# RPM build, run it with the same RUSTFLAGS (or set RUSTFLAGS yourself here).
export RUSTFLAGS="${RUSTFLAGS:--C linker=clang -C link-arg=-fuse-ld=mold}"

if [[ "$SKIP_BUILD" != "1" ]]; then
  cargo fetch --locked
fi

# --- 5. Spec -----------------------------------------------------------------
SPEC="$(mktemp --suffix=.spec)"
trap 'rm -f "$SPEC"' EXIT

cat > "$SPEC" <<'EOL'
Name:     %{pkg_name}
Version:  %{pkg_version}
Release:  %{pkg_release}%{?dist}
Summary:  Lightning-fast and Powerful Code Editor written in Rust
License:  Apache-2.0
URL:      https://github.com/infinite-dev22/sparrow

%description
sparrow is written in pure Rust with a UI in Floem (which is also written in Rust).
It is designed with Rope Science from the Xi-Editor which makes for lightning-fast computation, and leverages OpenGL for rendering.

%build
%if 0%{?skip_build}
echo "Skipping cargo build (SKIP_BUILD=1), using existing binary"
%else
cargo build --profile release-lto --bin sparrow --locked
%endif

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
EXTRA_DEFS=()
[[ "$SKIP_BUILD" == "1" ]] && EXTRA_DEFS+=(-D "skip_build 1")

rpmbuild --build-in-place --noprep --noclean \
  -D 'debug_package %{nil}' \
  -D "pkg_name ${PACKAGE_NAME}" \
  -D "pkg_version ${VERSION}" \
  -D "pkg_release ${RELEASE_STAMP}" \
  "${EXTRA_DEFS[@]}" \
  -bb "$SPEC"

# --- 7. Collect output -------------------------------------------------------
# Start clean so target/rpm only ever holds the RPM from this run, and match on
# the unique release stamp so older builds of the same version are never copied.
if [[ "$NO_CLEAN" != "1" ]]; then
  rm -rf target/rpm
fi
mkdir -p target/rpm
find "$(rpm --eval '%{_rpmdir}')" -name "${PACKAGE_NAME}-${VERSION}-${RELEASE_STAMP}*.rpm" -exec cp -v {} target/rpm/ \;

RPM_FILE="$(ls -t target/rpm/${PACKAGE_NAME}-${VERSION}-${RELEASE_STAMP}*.rpm 2>/dev/null | head -n1 || true)"
[[ -n "$RPM_FILE" ]] || { echo "no RPM was produced" >&2; exit 1; }

echo
echo "Done: ${RPM_FILE}"
echo "Install/upgrade with:  sudo dnf install ${RPM_FILE}"
echo "Force same-version reinstall with:  sudo dnf reinstall ${RPM_FILE}"