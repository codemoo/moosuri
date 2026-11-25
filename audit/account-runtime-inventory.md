# Account runtime migration source inventory

This is the executable source-inventory milestone for
[isolate-account-runtime](../openspec/changes/isolate-account-runtime/implementation.md).
It does **not** complete task 3.2, migrate accounts, authorize cutover, decrypt keys,
authenticate journals, or prove an independently restored personal volume.

```sh
# Replay the latest supported source schema into disposable in-memory SQLite.
cargo run --locked -p xtask -- account-runtime-inventory > inventory.json
# Keep dev.30/schema81 verification independently available on newer source.
cargo run --locked -p xtask -- account-runtime-inventory --schema 81 > inventory81.json
# Inspect only an explicitly supplied consistent stopped/checkpointed COPY.
cargo run --locked -p xtask -- account-runtime-inventory --schema 81 --snapshot /path/to/offline-copy > snapshot-inventory.json
```

The default schema follows the newest source migration; unknown versions fail.
An explicit supported `--schema` replays only that pinned baseline. Schema81 has
202 application tables, including virtual FTS, schema82 has 203, schema83 has 204, schema84 has 207, schema85 has 208 and schemas 86–87 have 209. All seven have 34
tables with neither `tenant_id` nor `account_id`. SQLite internal tables and the
five FTS shadow tables are excluded using SQLite's actual table kinds, rather
than a name-prefix guess. Schema82 extends the frozen 81 classification with its
one exact Gmail server-resume table and migration hash. Schema83 adds the per-run private model-preview binding and receipt. Schema84 records the private Google app history, encrypted read-only grant and pending consent. Schema85 adds the encrypted thread-specific inbox result and its durable dispatch status. Schema86 adds administrator-assigned member models. A quarantined restore clears those assignments. The manual knowledge archive excludes the grants, consent records and inbox results.

Schema 91 has 216 application tables. The member workflow definition binds an owner, volume generation and routing epoch. A run retains source identifiers and revisions. Restore pauses recurring definitions. Inbox phase history contains no mail payload. [Schema 91 classification](account-runtime-schema91.json)

Schema 92 has 223 application tables. Conversation groups, tasks, actions, events, runs and saved report receipts retain the owning member. Restore closes pending searches and questions. Browser progress digests expire with their job. [Schema 92 classification](account-runtime-schema92.json)

Schemas 93–100 extend the same member volume with interpreted work, combined reports, recurring checks, scoped Gmail and Calendar actions, research outcomes, linked task history, connected briefs and public watches. Schema 100 classifies 241 application tables. The briefing and public-watch ledgers pin their source authority and scheduled occurrence; encrypted excerpts are bound to the member and grant. [Schema 100 classification](account-runtime-schema100.json) · [Ciphertext bindings](account-runtime-artifacts.json)

Schema 101 adds the optional won limit and its inclusive or exclusive comparison to the member's public watch. Existing content watches keep both fields empty. The inventory covers 241 application tables and 247 cross-owner foreign-key relationships. [Schema 101 classification](account-runtime-schema101.json)

The manifests pin every migration SHA-256, every application table SQL shape and
the complete non-shadow table/index/trigger/view fingerprint. Each table has an
explicit disposition, owner query or global/regeneration decision, key transition,
verification and failure action. The tool checks SQLite integrity, all declared
foreign keys, complete and unique owner-join coverage, exact AccountId/TenantId
pairs, and cross-owner references for 187 declared relationships in 81, 189 in 82, 190 in 83 and 196 in 84.
Deletion/vault ACK and external auth-root cleanup records retain durable installation responsibility, and recovery spend dispatches remain non-rewindable even when owner-indexed.
Parent-only broker auth/binding/repository/mail-grant rows resolve through their
actual device/binding parents. Personal compatible providers partition by the
immutable ownership table; personal model selection cannot borrow another
account's provider, and installation selection cannot promote a personal provider.
Invitation actor/target records remain installation-private, rather than being
assigned to whichever account appears first. FTS and local counters have explicit
rebuild actions; global journals require new authenticated recovery anchors.

The artifact registry lists all 11 names admitted by the existing Store data-root
boundary, six external storage/authority boundaries and 14 encrypted-field binding
groups with the original AAD implementation and required rekey/reauthentication
rules. Inspection emits non-null ciphertext counts and file sizes; it never opens
key or journal contents. External browser/GWS/Claude/vault/receiver paths are
explicitly **not discovered or verified** by this milestone.

`--snapshot` refuses unknown entries, symlinks, WAL/SHM (including empty sidecars)
and incomplete cutover staging. The operator must make a complete consistent
stopped/checkpointed copy first. The database is opened through an immutable
read-only SQLite URI and `query_only` transaction; no Store startup, writer lock,
runner, provider, migration on disk, network or external action is invoked. Do
not modify the supplied snapshot concurrently. An immutable SQLite connection
cannot establish whether a live database was copied consistently.

Output contains public schema/classification, counts, fingerprints and reasons;
no account IDs, email addresses, row payloads, credentials or raw SQLite errors.
An unknown schema/table/file, missing/ambiguous owner or inconsistent relationship
fails closed. Success always has `cutover_ready:false`. Missing optional journal
or key files are presence information, not a recovery-completeness assertion.
Embedded JSON references, undeclared semantic relationships, payload/file digest
reconciliation, credential binding proof, authenticated owner genesis, drain and
single-generation execution remain explicitly pending.

Validation on isolated synthetic data (2026-11-19):

- `cargo test --offline --locked -p xtask account_runtime_inventory -- --nocapture`:
  13 passed, including source81/82/83 replay, parent joins, mismatched/missing owners,
  valid-FK cross-account provider misuse, private-provider promotion, source hash
  drift, unknown table/version/index/shape, incomplete classification, and immutable
  snapshot/no-sidecar/symlink/unknown-file failure boundaries.
- Schema81–85 CLI runs passed against pinned public migrations.
  Source-only replay contains 10 bootstrap rows; these are not user data acceptance.
- Formatting check passed. No production data, credentials or services were used.

[Offline manual-knowledge extraction](account-runtime-extraction.md) preserves one owner’s manual knowledge in an inert archive for synthetic restore rehearsals.
