#!/bin/sh
set -eu
test "$(cat /etc/moosuri-kiri-lab-marker)" = SYNTHETIC_DISPOSABLE_VM
for label in a b; do
 test ! -e /srv/moosuri-lab/$label.ext4 || { echo 'Refusing to overwrite existing synthetic filesystem' >&2; exit 1; }
done
# Guest only: two hard-cap filesystems. Never point this at real account data.
mkdir -p /srv/moosuri-lab/shared
printf 'public-readonly-module\n' >/srv/moosuri-lab/shared/module
for pair in a:10001 b:10002; do
 label=${pair%:*}; uid=${pair#*:}
 truncate -s 32M /srv/moosuri-lab/$label.ext4
 mkfs.ext4 -q -F -m 0 /srv/moosuri-lab/$label.ext4
 mkdir -p /srv/moosuri-lab/$label
 printf '/srv/moosuri-lab/%s.ext4 /srv/moosuri-lab/%s ext4 loop,nodev,nosuid,noexec 0 0\n' "$label" "$label" >>/etc/fstab
 mount /srv/moosuri-lab/$label
 chown "$uid:$uid" /srv/moosuri-lab/$label
 chmod 700 /srv/moosuri-lab/$label
done
mkdir -p /etc/systemd/system/kiri.service.d
cat >/etc/systemd/system/kiri.service.d/lab-storage.conf <<'UNIT'
[Unit]
RequiresMountsFor=/srv/moosuri-lab/a /srv/moosuri-lab/b
[Service]
ExecStartPre=/usr/bin/mountpoint -q /srv/moosuri-lab/a
ExecStartPre=/usr/bin/mountpoint -q /srv/moosuri-lab/b
UNIT
systemctl daemon-reload
