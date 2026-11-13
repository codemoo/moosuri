# Change: Bounded independent encrypted client archive

## Why
F50 needs a user-local encrypted archive that remains usable without the original service, while preserving worker lifetime, owner binding, immutable ciphertext retries and historical-wrap limits.

## What Changes
Add a bounded archive v1 authenticated wrapper around existing v2 encrypted headers/items, full local authentication before preview, duplicate/row selection and new-ID reencrypt import into the current unlocked vault. Reuse exact existing Put/CAS receipts. Add React download/load controls and synthetic core/Worker interruption checks.

## Impact
Only client crypto/WASM/React and documentation change. No service/schema, new dependency, plaintext export, original-server request, standalone backup/restore, old-v1 migration, credential broker or full-vault completion claim. See [archive contract](../../../docs/vault-archive.md).
