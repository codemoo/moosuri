# Separate vault service and browser client

This increment adds a usable v2 vault to the native core: a separate Rust ciphertext process and private SQLite/journal, a React client, and an actual wasm32 Rust module Worker. The crypto/service/client review findings and root integration review are closed; targeted Linux and actual Chrome verification passed. This is a development preview, not a deployment or mobile release. The later [bounded encrypted client archive](vault-archive.md) adds independent file export/import. The later [sealed-current service recovery](vault-service-recovery.md) adds bounded offline checkpoint/restore. The experimental [native credential broker](native-credential-github.md), [pinned historical recovery](vault-historical-recovery.md), [legacy v1 import](vault-v1-migration.md) and [separate component package delivery](component-packages.md) have bounded implementations. Actual off-host deployment, official signed artifacts and device acceptance remain open. [Account lifecycle delivery](account-management.md) adds deletion/outbox separately.

## Browser acceptance on Linux

On 2026-11-21 KST, six browser journeys passed on Ubuntu 22.04 with Chrome for
Testing, the standard assistant, the separate vault service and the Linux-built
WASM Worker. They covered launching the authenticated vault, confirming the
recovery key, saving and revealing a login, editing and searching, rejecting
a wrong password then unlocking with the recovery key, and cancelling then
confirming deletion across a reload. After building both binaries with their web
assets and installing Playwright Chromium, run
`python3 vault-web/tests/browser-fullstack.py target/release/moosuri target/release/moosuri-vault-service`.
The runner creates a private synthetic account and two loopback origins. [Browser test](../vault-web/tests/browser-fullstack.py)

## Admission and revocation

The assistant issues a 30-second single-use POST handoff from the current unrestricted session. Its digest is bound to account, tenant, session digest, auth epoch (including valid bootstrap epoch zero), installation and exact configured vault origin. The browser submits a hidden form; no handoff appears in a URL or browser storage. The assistant HTML shell preserves its Origin with `strict-origin` when the vault is configured; the vault requires that exact Origin for exchange. Ordinary assistant pages keep their existing policy when the vault is absent.

A dedicated authenticated loopback Rust adapter consumes the handoff and issues an independent grant. The vault stores a digest of its own random HttpOnly host-only cookie, never the assistant master cookie or assistant DB. HTTPS uses the `__Host-` cookie prefix and Secure/SameSite=Strict. Vault and assistant must use different cookie hosts; changing ports is insufficient. Both origins are installer settings; development uses separate localhost/127.0.0.1 hosts.

Every private read, prepare, reconciliation, cancel and local Worker command checks the underlying assistant session online. Logout, suspension, password reset/epoch change, session expiry and installation restore fence new admissions. Adapter failure fails closed; the Worker polls current authority every five seconds and terminates on failure. In-flight bytes already admitted cannot be recalled. UI lock/logout/account change/idle terminates its Worker, aborts requests and invalidates callback generations. A held reveal response is discarded after lock.

Ciphertext mutation linearizes at the assistant writer transaction that validates the current grant and records the exact operation ID/digest. A revocation committed first denies the mutation. A commitment admitted first may settle ciphertext afterward. The vault then serializes its local CAS adoption; competing candidates can conflict after admission. This is neither distributed all-or-nothing account deletion nor a generic token introspection API. The internal adapter accepts only exchange, validate, exact commit and close, rejects browser Origin/cookies, and requires a random 256-bit deployment key.

## Storage and interruption

The vault owns a private 0700 directory, 0600 regular files, exclusive process lock, schema1 SQLite WAL/FULL and a bounded fsynced hash-chain intent journal. DB identity/sequence/head must exactly match the complete journal. Missing, truncated, ahead or behind journal fails closed; no empty replacement or automatic restore recovery is fabricated. A crash between journal fsync and DB adoption requires operator investigation; this increment deliberately does not replay/adopt ambiguous intents.

Headers, encrypted item revisions and immutable operation candidates are retained. Exact CAS covers current header, inventory and item revision. Reservations retain nonce domain/generation/ciphertext commitments even for cancelled candidates; deleted item IDs cannot revive. Key rewrap retains item bytes; root rotation pins the complete sorted inventory and increments every item generation/revision. Limits are 256 live items, 32 outstanding candidates, 32MiB request/response, 256MiB retained candidate payloads per owner and 1GiB journal. Reaching a limit fails closed; retention compaction/archival remains future work.

Prepare never activates a key candidate. The real storage receipt includes operation ID, canonical payload digest, exact adopted header and ordered inventory digests. The Worker validates all of these and a matching core storage commitment before activation. If another tab has already moved the current head, the saved outcome is acknowledged but the Worker is terminated and a fresh unlock is required. An ambiguous ACK freezes editing and reconciles the same immutable operation, without encrypting it again. Cancel persists a tombstone and reservations, including when it wins before a delayed prepare. Closing the browser drops local pending keys; a later connection can see and cancel inactive server candidates. No lost in-memory key material is recoverable from the server.

## Trusted client

`moosuri-vault-wasm` builds on Rust1.89 and wasm-bindgen0.2.130 with native entropy disabled. Browser `crypto.getRandomValues` is the only injected entropy source; an unavailable CSPRNG returns a fixed failure. The linker caps WASM memory at384MiB. Rust owns roots, password/recovery work, decrypted items, CSV and pending key state in a dedicated module Worker. React receives summaries, edit fields and explicitly selected reveals/copies; it never receives root keys or a general decrypt capability. Selected TOTP codes are computed with Worker time, refreshed on rollover/focus and recomputed for copy. Native RFC vectors and actual browser Worker59→60 rollover are separate evidence.

The UI supports setup with recovery verification; password/recovery unlock; login/API-key/note CRUD; local search, favorites/expiry, rejection-sampled generated passwords, timed reveal/copy; password/recovery rewrap and full root rotation. CSV stays in Worker memory: generic username selection, row selection/duplicate flags, cancellation, stable encrypted batches, interrupted partial commit and same-operation reconciliation. Key changes remain blocked until the current import is closed, so a sealed remaining batch cannot cross a key generation. The browser cannot erase the source CSV from disk; the UI explains manual source cleanup. No vault service worker, IndexedDB, localStorage, sessionStorage or Cache API persistence is created. Clipboard, JS/OS copies and malicious replacement of server-delivered client code remain explicit limits. A separate origin and CSP are not signed independent-client distribution.

## Build and operator integration

Build the normal assistant React bundle, then run `tools/build-vault-web.sh` with Rust1.89, the wasm32-unknown-unknown target, Node22.12+ and wasm-bindgen0.2.130 on PATH. The script builds actual Rust WASM and React; build the Rust binaries afterward to embed both public bundles. Do not rebuild hashed dist assets concurrently with cargo/rustdoc. Generated WASM/glue and dist are ignored and reproducible; no personal data is bundled.

Configure assistant `serve --vault-origin https://vault.example.test --vault-adapter-bind 127.0.0.1:3211 --vault-adapter-key-file ABSOLUTE_PRIVATE_FILE`. Run `moosuri-vault-service --data-dir ABSOLUTE_PRIVATE_VAULT_DIR --origin https://vault.example.test --assistant-origin https://assistant.example.test --adapter-url http://127.0.0.1:3211/internal/v1/vault --adapter-key-file ABSOLUTE_PRIVATE_FILE`. Both servers bind loopback behind operator-managed HTTPS proxies. The key files must be owner-only regular files; provision separate owner-readable copies of the same random key for the two OS identities. Run the vault under a separate OS UID with separate filesystem rights. Two processes under one UID are a synthetic test arrangement, not an independent secret boundary. Service installation/TLS/UID provisioning is not automated or deployed here.

Assistant migration0024_vault_authority.sql follows media21, chat schedules22 and push23. Normal admission and sealed restore both migrate to schema24. Restore clears handoffs/grants and preserves only non-authorizing operation commitments; an old grant cannot survive restored enrollment. A late migration failure rolls back the whole migration and can retry without replacing the existing push tables. The independent vault DB starts at1 and must not be added to the assistant backup file list. Account destruction and two-DB outbox now follow [account management](account-management.md); delivery remains pending until this service acknowledges its durable lifecycle event. Ordinary assistant operation remains available when no vault origin/key is configured.

The minimal core additions are the owned `ClientVault` pending-state adapter and public ciphertext-only `storage_commitment`. Existing AEAD/KDF/AAD/wire formats were not replaced. Independent review covered these additions and the Rust→JS serialization copies; storage receipts are trusted-adapter assertions, not signatures or remote rollback proofs.

## Current key lifetime fence

The authenticated vault-local `/session/current` response contains `{owner, head}`; head is only the current header SHA256/revision/envelope epoch/root generation, not key material or header envelopes. Before new local secret work, after an asynchronous file read and again before emitting a result, the Worker compares that commitment to its admitted head. Password rewrap, recovery rewrap and root rotation all retire old honest-client Workers. Background checks run through the same serialized queue as commands. A changed head frees the engine, aborts transport and clears UI state. Inventory-only item changes do not unnecessarily revoke the shared key lifetime.

The final authenticated header read is the output admission point. If a peer change commits after that read, already-admitted bytes can still arrive; neither the protocol nor browser can retroactively erase them. A delayed initial check followed by a peer change is stopped by the final check, and every subsequent new operation checks the current head again.

An own committed key change with a lost ACK has a narrowly scoped exception: Rust verifies the exact locally owned operation ID/canonical payload/header/commitment/inventory receipt against the current own candidate head, without activating or admitting any returned header. This allows only settlement/background retention; ordinary secret-use paths still require the admitted head. Activation remains behind exact acknowledgement and current-state validation. An unrelated peer head retires the Worker and leaves persisted candidates/receipts intact.

Independent per-item CAS can validly add another item under the same unchanged header. When an exact committed receipt includes that additional inventory, the Worker verifies its own item effect in the exact current state and reports the save before requiring a fresh unlock. It does not retry an already committed operation indefinitely merely because its cached inventory was older.

## Lifecycle delivery

Service schema2 adds owner lifecycle tombstones and exact event receipts; schema1 roots migrate transactionally after existing DB/journal validation. The assistant adapter additionally supports authenticated service-only `lifecycle_next` and `lifecycle_ack`. No browser cookie, user key or private content enters that protocol. The service polls one event every two seconds, appends/fsyncs its lifecycle intent, purges only the matching owner and then ACKs the same event. Lost ACKs are idempotent. Deletion is permanent across later epochs and service recovery replay. Historical encrypted journal/checkpoint bytes follow retention policy; active DB deletion is not physical media secure erase.

## Native credential freshness

The experimental [native GitHub broker](native-credential-github.md) uses the same
vault-local session/current-head and full ciphertext state APIs. Its own native
vault-core root and selected-item pin are separate from the browser Worker. It
checks current owner, authenticated header/inventory and selected ciphertext again
after decrypt and before fixed adapter entry. Transient unavailability suspends
and retains a valid in-memory unlock; a changed item/header or rejected session
retires it and requires native reconnection. No master password enters either HTTP
service. An archive copy remains an explicit snapshot and is never a freshness
fallback for the live connection. The final fresh read is the admission point;
a later peer change cannot retract an already-entered provider request.
