# Independent encrypted client archive

The separate vault React client can export its unlocked v2 inventory as an encrypted JSON file, then import selected items into an unlocked current vault without contacting the original server. This is a bounded development increment for F50. It is not a standalone service DB/journal backup or restore, old-v1 migration, signed client distribution, credential broker, or completion of the full vault roadmap.

Choose **암호화 파일 내보내기** in the unlocked vault. The download name is always `moosuri-vault.encrypted.json`; item titles are never used in filenames. Preserve the file and the password or recovery key that protected the source vault when it was exported. In another unlocked vault, choose **암호화 파일 가져오기**, select the file, enter that source password/recovery key, inspect ordinal duplicate/changed/new status, select items and save. No source password, recovery key, root, field values, TOTP seed, or source archive goes to either server. Only new destination ciphertext and its normal immutable operation receipts use the current service.

The original owner, tenant, vault/item IDs, revisions, generation, KDF/envelope metadata, item count and ciphertext lengths are visible in the archive. All field values, including title and TOTP, remain encrypted. The generic filename prevents title leakage from the application; browser download history and user renaming are outside that guarantee. Import preview returns ordinal statuses and counts only, without titles, secrets or plaintext fingerprints.

## Archive v1 contract

The exact JSON object has `format: "moosuri-encrypted-vault-archive"`, `version: 1`, `count`, source v2 `header`, ordered source `items`, and canonical base64 `authentication`. Unknown/duplicate fields, trailing JSON, unsupported formats, noncanonical base64, malformed binding/revisions/generations, source duplicate IDs, count mismatch, more than256 items, header over16384 bytes, item over100000 bytes or total input over26MiB fail closed. Both source envelopes must satisfy the one already admitted Argon2id13/HKDF policy before any KDF runs. No new cryptographic dependency or primitive is introduced.

The 32-byte archive authentication key is HKDF-SHA256 with source root as input, source vault UUID bytes as salt and `moosuri-vault/archive/v1/auth` as info. The archive authentication is HMAC-SHA256 over this SHA256 transcript:

1. ASCII `moosuri-vault/archive/v1/manifest` followed by a NUL byte, then version byte1.
2. Canonical source header JSON length as big-endian u64, then its exact canonical JSON bytes.
3. Ordered item count as big-endian u64.
4. For every ordered item, canonical item JSON length as big-endian u64, then its exact canonical JSON bytes.

Canonical JSON here is the existing bounded Rust struct serialization, including struct field order. Import parses and validates all structure and sizes before the source KDF; it unwraps the source root locally and verifies existing header authentication, checks the manifest authentication in constant time, and authenticates/decrypts **every** item before installing import state or returning preview metadata. A valid manifest with a corrupted last payload still returns no preview. No caller-fabricated storage receipt or public raw-header admission authorizes archives. Core's private archive opener admits only a verified manifest and returns no source active vault to consumers.

Export loads a fresh validated current state and authenticates every item from its pinned inventory, then rechecks current header and inventory before releasing the encrypted result. A concurrent change detected during this snapshot returns a fixed conflict. A later mutation does not retroactively alter a completed coherent snapshot. The archive authenticates the supplied snapshot, not server completeness, newest-state history, rollback prevention or a malicious server's claims.

## Destination import and interruption

Duplicate and changed-account detection uses the same client-only canonical field/identity rules as CSV import, including duplicates inside the archive. Duplicate source IDs are invalid even when their ciphertext matches. Selected eligible rows receive fresh IDs distinct from source/current IDs, fresh data keys/nonces and revision1; all fields and TOTP are preserved and cryptographically bound to the current owner/tenant/vault and key generation. The current destination header is pinned. Same selection returns the same sealed batch; a different selection cannot replace a sealed batch.

Each item uses the existing exact immutable Put operation, payload commitment, CAS and durable receipt. A missing ACK keeps the identical pending operation and ciphertext; reconciliation advances committed count only after checking that exact receipt/current state. UI displays committed and remaining counts. Cancel before publication drops staging; cancel after publication reconciles the exact service receipt and preserves already committed items. Lock, logout, account change or destination key-generation/envelope change terminates the Worker, rejects pending callbacks and disposes late file/KDF responses. Lock is the interrupt control while source KDF executes. A new unlock recomputes duplicate status against loaded destination items; committed imports are then duplicates. Key changes and ordinary item mutations cannot replace local staged import state.

No plaintext CSV export, archive password replacement, archive recovery-key creation, external provider action or automatic archive upload is provided. Source plaintext exists transiently only in secret-owning core/Worker memory. Rust secret buffers are zeroized on drop; JS/browser/OS copies and immediate secure deletion cannot be guaranteed.

## Historical wrap and web-client limits

An archive preserves its source password and recovery wrappers. Later password changes, recovery-key replacement or root rotation cannot revoke old archive copies: their original password or recovery key continues to open them. A retained old wrapper plus its secret can expose the retained old root; root rotation protects newer ciphertext, not already exported ciphertext. Keep archives and their original unlock secrets separately protected, and dispose of obsolete files yourself when appropriate.

The current web app and source device remain trusted. A malicious server replacing served JavaScript can steal an unlock secret or unlocked contents. Separate origins, CSP, authenticated archive data and a Worker do not provide independently signed client distribution.

## Verification boundaries

Synthetic native core tests cover independent password/recovery roundtrip to a different owner, distinct IDs, duplicate/selection semantics, stable reencrypted retries and TOTP/favorite/expiry preservation; malformed/header/payload/order/count/duplicate/truncate/trailing-data/limits/KDF-policy/wrong-secret failure; valid manifest with corrupted final payload; cancellation and inventory mismatch. Native WASM-adapter tests exercise exact partial batch ACK reconciliation/cancel. Worker dispatch tests cover a late source-file read after destination key change and export inventory race; client tests cover lock during source/import work and delayed responses. Actual wasm32/React builds, root-owned browser/Linux integration and independent review passed. The [verification record](reviews/2026-11-13-vault-archive.md) separates actual browser encryption/download/import from mocks and records the remaining device and release limits.
