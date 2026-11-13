# Change: Native client vault cryptography/import core

## Why
The Rust product has required vault/credential security contracts but no working trusted-client cryptographic core. Implement a bounded native foundation without claiming the separate vault product or broker is usable.

## What Changes
Add a standalone Rust workspace library with explicit v2 owner-bound AES-GCM/Argon2id/HKDF envelopes, nonlogging secret types, committed-header and staged key-change admission, encrypted item operations, RFC6238 TOTP and bounded local Apple CSV preview/encrypted batches. No assistant migration or HTTP/model tool.

## Impact
F44–F51/F63/F69 remain only partially implemented. Native tests/dependency evidence cover this core; v1 compatibility, WASM/Worker/React, vault service/journal/archive/backup, independent client and broker need separate implementation/review. See [core design](../../../docs/vault-core-design.md).
