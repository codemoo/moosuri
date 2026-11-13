#!/bin/sh
set -eu
# Test target only. The CA authenticates an owned HTTPS fixture; it never disables
# certificate verification or enters the production runtime image.
case "${HOME:-}" in /profiles/*) ;; *) exit 1 ;; esac
for database in "$HOME/.pki/nssdb" "$HOME/.local/share/pki/nssdb"; do
    mkdir -p "$database"
    certutil -N --empty-password -d "sql:$database" >/dev/null 2>&1
    certutil -A -d "sql:$database" -n moosuri-synthetic-fixture-root -t 'C,,' -i /capsule/test-tls/ca.crt >/dev/null 2>&1
done
exec /usr/bin/chromium-real "$@"
