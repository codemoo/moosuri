# Change: Separate ciphertext vault and trusted browser client

## Why
F44–F49/F63 need a usable separate service and client after the native core, with real authority revocation, ciphertext persistence and interruption recovery. Native-only evidence does not establish a working browser vault.

## What Changes
Add Rust ciphertext/protocol/WASM crates, a React module Worker client, a narrowly authenticated assistant handoff/online authority adapter and reserved assistant migration0024 (standalone schema21). Add exact CAS/immutable operation receipts and journal/nonce retention. No credential execution, archive, backup/restore or legacy-v1 migration is claimed.

## Impact
R14/R16/R28 and F44–F49/F63 are advanced by this bounded increment; F50/F51/F64/F69 and independent-client/release gates remain open. See [service/client contract](../../../docs/vault-service-client.md). Independent crypto/service/client review and root schema integration are required.
