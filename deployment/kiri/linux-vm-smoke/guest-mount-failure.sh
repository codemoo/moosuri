#!/bin/sh
set -eu
test "$(cat /etc/moosuri-kiri-lab-marker)" = SYNTHETIC_DISPOSABLE_VM
base=/srv/moosuri-lab
unit=$(systemd-escape --path --suffix=mount "$base/a")
restore() {
 test ! -e "$base/a.ext4.offline" || mv "$base/a.ext4.offline" "$base/a.ext4"
 systemctl reset-failed "$unit" kiri || true
 systemctl start "$unit"
 systemctl start kiri
}
trap restore EXIT
systemctl stop kiri
systemctl stop "$unit"
test ! -e "$base/a.ext4.offline"
mv "$base/a.ext4" "$base/a.ext4.offline"
if systemctl start kiri; then echo UNEXPECTED_START; exit 1; fi
if systemctl is-active --quiet kiri; then echo UNEXPECTED_ACTIVE; exit 1; fi
if mountpoint -q "$base/a"; then echo UNEXPECTED_MOUNT; exit 1; fi
test ! -e "$base/a/owner"
printf 'PASS missing account filesystem prevents Kiri startup; empty mountpoint has no fallback owner data\n'
