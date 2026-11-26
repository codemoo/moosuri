#!/bin/sh
set -eu
# Synthetic fixture target only. Normal TLS verification remains enabled.
case "${HOME:-}" in /profiles/*) ;; *) exit 1 ;; esac
for database in "$HOME/.pki/nssdb" "$HOME/.local/share/pki/nssdb"; do
    mkdir -p "$database"
    certutil -N --empty-password -d "sql:$database" >/dev/null 2>&1
    certutil -A -d "sql:$database" -n moosuri-synthetic-google -t 'C,,' -i /capsule/test-tls/ca.crt >/dev/null 2>&1
done
exec /usr/bin/google-chrome-stable-real "$@"
