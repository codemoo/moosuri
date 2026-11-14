# Account deletion and vault lifecycle delivery — local evidence

This tranche adds permanent administrator deletion with exact revision/idempotent
receipt, immediate durable authentication revocation, owner-only active private
content/credential cleanup, and separate-vault journal/outbox/ACK delivery.
Self deletion and last-usable-administrator protections follow account management.
Administrators obtain no vault read or decrypt capability.

Synthetic local verification:

- Account store suite:8 passed, including scoped media/file/source-index/credential
  purge; another owner's content preserved; unknown outgoing mail state and model
  accounting retained; cleanup failure stays pending while login is revoked;
  same request retries cleanup; journal-before-DB interruption and old SQLite
  snapshot do not resurrect account data/passwords; operator recovery is denied.
- Vault service:lib3, HTTP2, storage5, recovery8 passed. Actual authenticated Rust
  adapter delivery stays pending during failure, then commits exact ACK. Tests
  cover lifecycle fsync crash-tail replay, idempotent event identity, scoped
  ciphertext removal, late commit/higher-epoch rejection and sealed checkpoint
  restore preserving deletion while retaining another owner's ciphertext.
- Account UI/password lifetime:9 passed; production web build/typecheck passed.
  UI distinguishes local cleanup, vault pending/ACK and historical retention.
- Existing authority/migration suite:4 passed after updating the expected vault
  table count for the new lifecycle outbox.

Initial shared-target stale core artifacts were invalidated before testing. A
new two-owner test initially reused one synthetic session cookie; fixtures now
use owner-specific cookies. The historical migration test initially expected
three vault tables; schema41 correctly adds a fourth. These were repaired before
the final passing runs. Existing ts-rs serde-attribute and bundle-size warnings
remain unrelated to this tranche.

No real account/provider deletion, production DB, external credential, collector,
cron or running operational service was used. Native evidence is macOS only;
independent security/integration review, Linux checks and publication belong to
the root integration gate. This is logical active-service deletion. Historical
encrypted vault intent/checkpoint/backup bytes and storage-media remnants remain
under retention/disposal policy; no secure media erase or provider-side remote
content deletion is claimed. Minimal opaque accounting/effect evidence survives,
and an unknown dispatched effect is never relabelled cancelled.
