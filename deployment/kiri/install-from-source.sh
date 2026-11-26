#!/bin/bash
# Thin source-build entry. Host preparation and all product setup are Rust commands.
set -euo pipefail
umask 077
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
unset DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH BUILDX_BUILDER
if [[ $EUID != 0 || $(uname -m) != x86_64 || ! -d /run/systemd/system ]]; then
    echo 'Run as root on an Ubuntu 24.04 amd64 systemd server.' >&2
    exit 1
fi
if ! /usr/bin/grep -qx 'ID=ubuntu' /etc/os-release || ! /usr/bin/grep -qx 'VERSION_ID="24.04"' /etc/os-release; then
    echo 'Ubuntu 24.04 is required.' >&2
    exit 1
fi
if [[ $# == 0 ]]; then
    echo 'Usage: sudo bash deployment/kiri/install-from-source.sh --public-origin https://assistant.example.com --vault-origin https://vault.example.com --certbot-email operator@example.com --accept-certbot-terms' >&2
    exit 2
fi
source_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
cd -- "$source_root"
for key in ${!GIT_@}; do unset "$key"; done
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_ATTR_NOSYSTEM=1
for directory in "$source_root" "$source_root/.git"; do
    owner=$(stat -c %u "$directory")
    mode=$(stat -c %a "$directory")
    if [[ ! -d $directory || -L $directory || ( $owner != 0 && $owner != "${SUDO_UID:-0}" ) ]] || (( (8#$mode & 8#022) != 0 )); then
        echo 'Refusing an unowned or shared source checkout.' >&2
        exit 1
    fi
done
if [[ -e .git/info/attributes || -L .git/info/attributes ]]; then
    echo 'Local archive attributes require review before building.' >&2
    exit 1
fi
revision=$(git -c "safe.directory=$source_root" rev-parse --verify HEAD)
if git -c "safe.directory=$source_root" ls-tree -r "$revision" | /usr/bin/grep -E '^(120000|160000) |[[:space:]](.*/)?\.gitattributes$' >/dev/null; then
    echo 'Source links, submodules and archive attributes require review before building.' >&2
    exit 1
fi
build_root=/var/cache/moosuri-source-build
if [[ -L $build_root || ( -e $build_root && $(stat -c '%u:%a' "$build_root") != 0:700 ) ]]; then
    echo 'Refusing an unowned or shared source build directory.' >&2
    exit 1
fi
mkdir -p -m 700 "$build_root"
stage=$(mktemp -d "$build_root/build.XXXXXXXX")
echo "Building source revision $revision; uncommitted files are excluded."
echo "Build and recovery directory: $stage"
# This is the only pre-Rust bootstrap. No host compiler, Node or member credentials.
DEBIAN_FRONTEND=noninteractive apt-get update </dev/null
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends docker.io docker-buildx ca-certificates </dev/null
systemctl enable --now docker.service </dev/null
mkdir -m 700 "$stage/source" "$stage/docker-config"
git -c "safe.directory=$source_root" -c core.attributesFile=/dev/null archive --format=tar "$revision" | tar -xf - -C "$stage/source"
docker --host=unix:///var/run/docker.sock --config "$stage/docker-config" build \
    --file "$stage/source/deployment/kiri/build-installation.Dockerfile" \
    --target bundle-tool --output "type=local,dest=$stage/tool" "$stage/source" </dev/null
"$stage/tool/xtask" kiri-host </dev/null
"$stage/tool/xtask" kiri-bundle --output "$stage/bundle" --revision "$revision" </dev/null
echo "Bundle ready. To resume setup, use: $stage/bundle/moosuri setup-kiri --bundle $stage/bundle [the same options]"
exec "$stage/bundle/moosuri" setup-kiri --bundle "$stage/bundle" "$@"
