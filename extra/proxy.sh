#!/bin/sh
set -eux

# This script is written to be as POSIX as possible
# so it works fine for all Unix-like operating systems

test_cmd() {
  command -v "$1" >/dev/null
}

# proxy version
sparrow_new_ver="${1}"
# proxy directory
# eval to resolve '~' into proper user dir
eval sparrow_dir="'${2}'"

case "${sparrow_new_ver}" in
  v*)
    sparrow_new_version=$(echo "${sparrow_new_ver}" | cut -d'v' -f2)
    sparrow_new_ver_tag="${sparrow_new_ver}"
  ;;
  nightly*)
    sparrow_new_version="${sparrow_new_ver}"
    sparrow_new_ver_tag=$(echo ${sparrow_new_ver} | cut -d '-' -f1)
  ;;
  *)
    printf 'Unknown version\n'
    exit 1
  ;;
esac

if [ -e "${sparrow_dir}/sparrow" ]; then
  sparrow_installed_ver=$("${sparrow_dir}/sparrow" --version | cut -d' ' -f2)

  printf '[DEBUG]: Current proxy version: %s\n' "${sparrow_installed_ver}"
  printf '[DEBUG]: New proxy version: %s\n' "${sparrow_new_version}"
  if [ "${sparrow_installed_ver}" = "${sparrow_new_version}" ]; then
    printf 'Proxy already exists\n'
    exit 0
  else
    printf 'Proxy outdated. Replacing proxy\n'
    rm "${sparrow_dir}/sparrow"
  fi
fi

for _cmd in tar gzip uname; do
  if ! test_cmd "${_cmd}"; then
    printf 'Missing required command: %s\n' "${_cmd}"
    exit 1
  fi
done

# Currently only linux/darwin are supported
case $(uname -s) in
  Linux) os_name=linux ;;
  Darwin) os_name=darwin ;;
  *)
    printf '[ERROR] unsupported os\n'
    exit 1
  ;;
esac

# Currently only amd64/arm64 are supported
case $(uname -m) in
  x86_64|amd64|x64) arch_name=x86_64 ;;
  arm64|aarch64) arch_name=aarch64 ;;
  # riscv64) arch_name=riscv64 ;;
  *)
    printf '[ERROR] unsupported arch\n'
    exit 1
  ;;
esac

sparrow_download_url="https://github.com/infinite-dev22/sparrow/releases/download/${sparrow_new_ver_tag}/sparrow-proxy-${os_name}-${arch_name}.gz"

printf 'Creating "%s"\n' "${sparrow_dir}"
mkdir -p "${sparrow_dir}"
cd "${sparrow_dir}"

if test_cmd 'curl'; then
  # How old curl has these options? we'll find out
  printf 'Downloading using curl\n'
  curl --proto '=https' --tlsv1.2 -LfS -O "${sparrow_download_url}"
  # curl --proto '=https' --tlsv1.2 -LZfS -o "${tmp_dir}/sparrow-proxy-${os_name}-${arch_name}.gz" "${sparrow_download_url}"
elif test_cmd 'wget'; then
  printf 'Downloading using wget\n'
  wget "${sparrow_download_url}"
else
  printf 'curl/wget not found, failed to download proxy\n'
  exit 1
fi

printf 'Decompressing gzip\n'
gzip -df "${sparrow_dir}/sparrow-proxy-${os_name}-${arch_name}.gz"

printf 'Renaming proxy \n'
mv -v "${sparrow_dir}/sparrow-proxy-${os_name}-${arch_name}" "${sparrow_dir}/sparrow"

printf 'Making it executable\n'
chmod +x "${sparrow_dir}/sparrow"

printf 'sparrow-proxy installed\n'

exit 0
