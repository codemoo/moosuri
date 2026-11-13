# Encrypted vault archive — 2026-11-13

The separate vault client exports an authenticated ciphertext file and imports
selected items using the source password or recovery key. The original service
is not contacted. All source items authenticate before preview; selected items
receive fresh IDs, keys and ciphertext bound to the current destination owner.
The existing immutable Put/CAS/receipt pipeline handles partial saves and lost
acknowledgements. There is no schema or dependency change.

Independent review accepted the exact18-file increment without a material
finding. The reviewer reconstructed the framed HKDF/HMAC manifest with a separate
Python implementation, tested duplicate JSON fields and checked late file reads,
key-generation changes, ordered item authentication and batch reconciliation.
Root integration preserved the published Calendar26 increment; only shared
documentation needed a merge.

## Verification

- macOS: native core16, WASM adapter4 and compile-fail4; Worker/client9;
  actual wasm32 build, generated glue and React production build; type checking,
  focused all-target clippy, formatting, plan links and strict OpenSpec44.
- Linux x86_64: core16, service HTTP/storage5, adapter4 and compile-fail4 passed
  against the hash-verified integrated source and final embedded vault bundle.
- Actual Chrome and both Rust services: create two synthetic source items and
  download the real encrypted file; no title, username, password, note or TOTP
  canary appeared in the exported bytes. The source session was retired before
  a different account created its empty destination vault.
- The destination rejected a modified manifest before preview or write. Password
  import preserved secret, TOTP, favorite and expiry; source IDs and ciphertext
  were not reused. Dropping the first real commit response still resulted in
  exactly two unique Put requests and two commits for two items. Recovery-key
  opening of the same file recognized both imported items as duplicates.
- Lock during source processing terminated the actual Worker after its two
  authenticated prechecks and before its final output check. Unlock preserved
  the committed items and did not restore a late import preview. At390×844,
  screenshots and layout bounds showed no horizontal overflow.

The first browser layout assertion incorrectly required document width to equal
viewport width; a vertical scrollbar made it15px narrower. The corrected check
rejects overflow, and the interrupted import journey passed again. No product
permission or cryptographic check was relaxed.

Only synthetic data was used. Source passwords, recovery keys and original
archive bytes were absent from destination network request bodies. These checks
do not prove worst-case26MiB behavior on a phone, newest-state completeness,
rollback prevention or signed client integrity. Later password/root changes
cannot revoke an old exported file. Standalone service backup/restore, old-v1
migration, credential broker, account deletion and production deployment remain
separate work; see the [contract](../vault-archive.md).
