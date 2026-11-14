# Local migration of an old v1 encrypted export

In an unlocked current v2 vault, choose **구 v1 볼트 파일 이전**, select an old authenticated JSON export, and enter its export-time password or `MR1-` recovery key. The device authenticates the complete manifest and every payload before returning ordinal new/changed/duplicate counts. Select eligible rows and save them with the existing destination import receipts. The original service is unnecessary. The original file is unchanged and must be retained or disposed of by its owner.

This is a user-local ciphertext-file migration for F50. It does not migrate an old database, server identity, session, journal or backup, admit a raw header/items dump, or establish that the file contains the original server's newest or complete inventory. The ordinary current archive path continues to reject old v1 input; only the explicit legacy route accepts it. Current encrypted archive version1 and old vault version1 are different formats.

## Accepted wire contract

The exact object is `{version:1,format:"musuri-vault",header,items,manifest:{nonce,ciphertext}}`. Unknown/duplicate fields and trailing JSON fail closed. UUIDs must have their original lowercase hyphenated spelling; alternate spellings are rejected instead of being normalized into authenticated data. Integers must be positive JavaScript-safe revisions/generations (`1..9007199254740991`). Base64 is padded standard canonical base64 except the URL-safe unpadded recovery key.

The header has `version,id,generation,envelope_epoch,revision,envelopes`. Exactly two distinct envelope IDs and one envelope of each kind are required, in either order. Each envelope has `id,kind,created_version:1,generation,salt,kdf,nonce,ciphertext`; generations must match the header. The password KDF is exactly `{algorithm:"argon2id13",memory:67108864,iterations:3}`: Argon2id v0x13, lanes1, 64MiB, three passes, output32. Passwords are UTF-8 bytes without Unicode normalization, limited to1024 bytes. Recovery uses `MR1-` plus43 canonical URL-safe characters encoding32 bytes, and `{algorithm:"hkdf-sha256"}`. Its HKDF-SHA256 uses the envelope salt16 and info `musuri-vault/v1/recovery/<vault-id>/<envelope-id>`. AES-256-GCM uses nonce12, tag16 and a48-byte wrapped root.

Envelope AAD is canonical JSON `{domain:"musuri-vault/v1/envelope",vault_id,envelope:{id,kind,created_version,generation,salt,kdf}}`. Each item has `version:1,id,generation,revision,key_nonce,wrapped_key,nonce,ciphertext`; IDs and key-wrap nonces must be unique. Wrap/payload AAD is `{domain:"musuri-vault/v1/item/key" or "musuri-vault/v1/item/payload",version:1,vault_id,item_id,generation,revision}`. Canonicalization recursively sorts ASCII metadata keys lexicographically and preserves array order, with JSON scalar encoding. Full header serialization includes both envelopes and their ciphertext, without omitting fields.

The manifest key is HKDF-SHA256 of the root, salt=manifest nonce12, info `musuri-vault/v1/archive-key/<vault-id>/<generation>`. Its AES-GCM AAD is `{domain:"musuri-vault/v1/archive",version:1,header}`. Decrypted manifest bytes must exactly equal canonical `{version:1,count:items.length,ids:[ordered item IDs],digest:<padded standard-base64 SHA256(canonical({header,items}))>}`. Authenticating a supplied inventory does not prove source-server completeness or latestness.

Decrypted item fields are `kind` (`login`, `api_key`, `note`), `title`, `username`, `url`, `secret`, `notes`, `favorite`, `expires_at`, and optional `totp` (absence becomes empty). Unknown/duplicate fields fail. All fields are preserved without truncation. Current byte caps apply: title512, username4096, URL8192, secret16384, notes32768, expiry128, TOTP4096, total serialized plaintext65536. A source value outside these narrower destination limits rejects the entire import before preview.

## Bounds, destination and interruption

The complete input is limited to256 items and26MiB, matching the existing current import capacity. These are intentionally narrower than the old1000-item/96MiB export bounds. Oversized input, malformed metadata/base64, unsupported KDF parameters, mismatched generations or duplicate IDs/nonces reject the whole file before any source KDF; nothing is silently truncated or partially previewed. Header/item serialized caps are16384/100000 bytes. Payload ciphertext is16..65552 bytes; wrapped item key is48 bytes. The manifest ciphertext is capped at262144 bytes. JSON parsing, memory allocation and the password KDF are still bounded work, not a constant-time input rejection promise.

Only the existing authenticated-field import stage is shared. Selected rows receive fresh IDs distinct from all source/current IDs, revision1, independent keys/nonces and the current v2 owner/tenant/vault binding. No old header or authority becomes an active destination vault. Identical selection returns the same sealed batch; another selection cannot replace it. Existing immutable Put/CAS/receipt reconciliation handles missing ACKs. Cancel drops remaining staging and preserves committed items. Lock terminates the Worker; late reads/responses and destination-generation changes cannot reinstall import state. A fresh unlock recomputes duplicates against committed destination items.

The source file/password/recovery key and plaintext never enter network requests. Only current destination ciphertext and normal operation receipts leave the Worker. Rust secret buffers are zeroized on drop; JavaScript/browser/OS copies cannot be guaranteed erased. The served client and device remain trusted. Original export-time wrappers continue to open old copies despite later password/recovery changes or root rotation. This increment adds no dependency, schema, cache, crypto primitive or production operation.

## Contract and vector evidence

The implementation was freshly authored from the actual old export contract, with read-only access limited to source types/validation/engine/client and adjacent export call sites. No old runtime/tests, personal exports, DBs, keys or credentials were copied or executed. Source SHA256 at contract extraction:

| Old source under `vault-web/src/` | SHA256 |
| --- | --- |
| `crypto/types.ts` | `4fb18b180fc7b83cdc1ee10300224deea9cf550679fec1ceca3cb566974f1135` |
| `crypto/validation.ts` | `e0347fe3714f23f558eb67575f60d60cfc26d91d1ce940f0637192e510169e4d` |
| `crypto/engine.ts` | `4e5076f2b2af01b6b6363d93d30f9b996bd7ca3ab6fdd00c472b225b40e86417` |
| `crypto/client.ts` | `583e855dc390264ca2674b84596929f5a8273c90857af98d2ab2b05e1ceeae3c` |
| `useVault.ts` (export call site) | `01ec0284604e710e9bc435015311531c70bbe1555cf573ea634eebc247cf2e6d` |

Committed fixtures under `crates/moosuri-vault-core/tests/fixtures/legacy-v1-*` are explicitly synthetic and expose only synthetic keys/plaintext. An independent contract-authored Node WebCrypto AES-GCM/HKDF/SHA256 harness generated and verified them using installed PyNaCl's libsodium Argon2id13. Versions: Node22.22.0, OpenSSL3.5.4, PyNaCl1.5.0, algorithm constant2; the installed binding does not expose its libsodium backend version, so that version is unknown. No test dependency was added. Archive SHA256 is `08751e0b1e093696da08776152283d5b8409447dcc64aee74ceaa430fe14692c`.

Rust reproduces the exact independent root wrappers, item-key wraps, payloads, manifest and canonical bytes, then imports both password/recovery routes into a different v2 destination. The fixture includes reversed envelope order, generation/revision above1, composed/decomposed Unicode and emoji, TOTP and an absent optional TOTP. Focused tests cover hostile pre-KDF structure, wrong secret, order/deletion tamper, a valid manifest with a corrupted final payload, raw-dump/no-fallback rejection, cancellation, stable reencrypt selection, duplicate detection and exact receipt/cancel behavior. Worker tests cover explicit local dispatch, source-secret network exclusion and late reads after destination generation changes. Native tests, Clippy, actual wasm32 build and React checks are local evidence; independent review, Linux integration and actual browser gates are recorded by the integrating owner and must not be inferred from these local checks. No real personal export or mobile performance was tested.

Integrated independent, Linux and actual Chrome acceptance is recorded in the
[acceptance review](reviews/2026-11-15-vault-v1-migration.md). Real historical
operator exports and mobile performance remain separate.
