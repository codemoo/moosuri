#!/usr/bin/env bash
# Moosuri's first stage: HTTPS -> pinned archive SHA-256 -> signed Rust installer.
# Product setup, accounts and service management live in Rust.
set -euo pipefail

moosuri_install() {
  case "${1-}" in -h|--help)
    cat <<'HELP'
Moosuri / 무수리 installer
  --language en|ko   English / 한국어
  --plain            Numbered menus / 번호 메뉴
  --no-browser       Show the URL / 브라우저를 자동으로 열지 않음
Run without sudo on macOS, Linux or inside WSL2.
macOS·Linux·WSL2에서 sudo 없이 실행하세요.
HELP
    return 0;; esac
  if [ "$(id -u)" = 0 ]; then
    printf '%s\n' 'Run without sudo. / sudo 없이 일반 사용자로 실행하세요.' >&2; return 1
  fi
  if ! { true </dev/tty; } 2>/dev/null; then
    printf '%s\n' 'An interactive terminal is required. / 대화형 터미널에서 실행해주세요.' >&2; return 1
  fi
  local platform machine target archive_sha version release_key
  platform=$(uname -s); machine=$(uname -m)
  case "$platform" in
    Darwin)
      if [ "$(sysctl -in sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then machine=arm64; fi
      case "$machine" in arm64|aarch64) target=aarch64-apple-darwin;; x86_64|amd64) target=x86_64-apple-darwin;; *) target=unsupported;; esac;;
    Linux)
      case "$(uname -r | tr '[:upper:]' '[:lower:]')" in
        *microsoft*standard*|*wsl2*) :;;
        *microsoft*) printf '%s\n' 'WSL2 is required. In PowerShell: wsl --set-version <distribution> 2' 'WSL2가 필요합니다. PowerShell에서 배포판을 WSL2로 전환하세요.' >&2; return 1;;
      esac
      if ! getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        printf '%s\n' 'Use a glibc Linux distribution (Ubuntu/Debian/Fedora). Alpine/musl is not supported.' 'glibc 기반 Linux를 사용하세요. Alpine/musl은 지원하지 않습니다.' >&2; return 1
      fi
      case "$machine" in aarch64|arm64) target=aarch64-unknown-linux-gnu;; x86_64|amd64) target=x86_64-unknown-linux-gnu;; *) target=unsupported;; esac;;
    *) target=unsupported;;
  esac
  # BEGIN GENERATED RELEASE PINS (updated only with a verified preview publication)
  version=0.1.0-dev.2
  release_key=UNPUBLISHED
  case "$target" in
    aarch64-apple-darwin) archive_sha=UNPUBLISHED;;
    x86_64-apple-darwin) archive_sha=UNPUBLISHED;;
    x86_64-unknown-linux-gnu) archive_sha=UNPUBLISHED;;
    aarch64-unknown-linux-gnu) archive_sha=UNPUBLISHED;;
    *) printf '%s\n' "Unsupported platform / 지원하지 않는 환경: $platform $machine" >&2; return 1;;
  esac
  # END GENERATED RELEASE PINS
  if [ "$archive_sha" = UNPUBLISHED ]; then printf '%s\n' 'Preview packages are not published yet. / 설치 패키지를 아직 공개하지 않았습니다.' >&2; return 1; fi
  local dependency
  for dependency in curl tar mktemp; do
    command -v "$dependency" >/dev/null 2>&1 || { printf '%s\n' "Install $dependency first. / $dependency 설치가 필요합니다." >&2; return 1; }
  done
  local stage physical_tmp actual url
  physical_tmp=$(cd "${TMPDIR:-/tmp}" && pwd -P)
  stage=$(umask 077; mktemp -d "$physical_tmp/moosuri-install.XXXXXXXX")
  trap "rm -rf -- $(printf '%q' "$stage")" EXIT
  umask 077
  url="https://github.com/codemoo/moosuri/releases/download/v$version/moosuri-$version-$target.tar.gz"
  printf '%s\n' "Downloading Moosuri $version ($target)… / 무수리를 내려받습니다…"
  curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 15 --max-time 300 --retry 2 --output "$stage/package.tar.gz" "$url"
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$stage/package.tar.gz"); actual=${actual%% *}
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$stage/package.tar.gz"); actual=${actual%% *}
  else printf '%s\n' 'SHA-256 utility is required. / SHA-256 검사 도구가 필요합니다.' >&2; return 1; fi
  if [ "$actual" != "$archive_sha" ]; then printf '%s\n' 'Download verification failed; nothing was executed. / 검증 실패: 다운로드한 파일을 실행하지 않았습니다.' >&2; return 1; fi
  mkdir "$stage/bundle"
  tar -xzf "$stage/package.tar.gz" -C "$stage/bundle"
  chmod 700 "$stage/bundle" "$stage/bundle/moosuri"
  chmod 600 "$stage/bundle/manifest.json" "$stage/bundle/manifest.ed25519"
  printf '%s\n' "$release_key" > "$stage/pin"
  "$stage/bundle/moosuri" installation verify --bundle "$stage/bundle" --trust-key "$stage/pin"
  "$stage/bundle/moosuri" setup --bundle "$stage/bundle" --trust-key "$stage/pin" "$@" </dev/tty
  rm -rf -- "$stage"
  trap - EXIT
}
# Keep the call at the end: a truncated download cannot execute a partial function.
moosuri_install "$@"
