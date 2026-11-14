# Explicit private transfer import — selected evidence

The offline Rust CLI accepts only the documented normalized owner-selected ordered
JSON transfer contract and creates a new SQLite staging root. No legacy database,
operating export, original source schema, credentials, provider, model, mail or
production service was accessed. No collector/cron/source cursor changed. CI stays
off. This is a bounded transfer implementation, not evidence that an existing
private deployment was migrated or that aggregate P9 is complete.

## Behavior and trust boundary

Sources preserve explicit external identity/version/speaker/observed/valid times;
memory and document histories use actual normal Store mutation rules. Explicit
ordered events preserve reference/revision context without weakening current
reference admission. Correction/forget/deletion propagates normal journal and
dependency withdrawal. A rejected correction poisons and withdraws its prior staged
object rather than publishing a stale assertion. Every input event has outcome,
canonical parsed-row hash, index/sequence and fixed reason. Exact bundle bytes are
separately hashed and retained unchanged. Other owners and unsupported work,
credentials, media/vault/usage/settings/events quarantine without enabling them.

One selected legacy owner maps to a freshly enrolled admin/tenant. No legacy role,
password hash/session/provider/connection/work/budget is copied. Imported collections
end disconnected, authority tables are asserted empty and the internal import login
is revoked before publication. Partial construction uses the existing blocked
restore-stage prefix with a fresh internal admission token, so an interrupted stage
cannot be opened by normal Store/serve/recovery. Reports publish first; complete
SQLite root publishes last with same-parent native no-overwrite rename and fsync.
Existing destinations reject. A leftover report alone is never data completion.

Separate report/quarantine files and unchanged original bundle are required private
reconciliation evidence. Rejected raw payload is not redundantly copied into active
DB/logs. Bounded mapping/report chunks are ordinary editable private document
artifacts, included in normal encrypted DB exports; they are not immutable provider
receipts or latestness proof. Source provider provenance is not fabricated.
Memory/document Store created/updated timestamps are import-time values; only
explicit source observed time and validity intervals are preserved.

## Native gates on2026-11-15

Pinned Rust1.89, clean baseline594235d/schema49:

- `cargo test --locked -p moosuri-store legacy_import::tests -- --nocapture`:2 passed,2.32s. Real Store ordered source revision1/2, source-grounded memory revision1/2, current and historical reads, dependency document withdrawal, independent document revision1/2, terminal memory forget and body-less tombstone, invalid correction withdrawal, missing dependent reference, unsupported credential/work payload and foreign-owner quarantine. All15events are accounted, input unchanged, private reject payload absent from quarantine report. New user cannot read chosen owner's imported content. Restart, FK/integrity checks and actual stopped age setup/export pass with SQLite report artifacts retained.
- The second Store gate rejects malformed private input before output creation and proves a private partial import stage cannot be ordinarily opened after its writer closes.
- `cargo test --locked -p moosuri --test private_import -- --nocapture`:1 passed,2.67s. Actual CLI creates a fresh account/root, imports selected memory, accounts unsupported old run without printing its synthetic secret, reports quarantine counts, survives Store reopen/login, refuses overwrite/retry without changing reports and produces a real encrypted export.
- `git diff --check`:passed.

Initial compile identified a helper visibility and collection-revision call mismatch;
these were corrected before the successful gates. The first implementation tried a
nested data rename through a helper that supports same-parent rename only; the
selected Store gate caught this before publication acceptance. Data now stages as
the direct sibling root under its own blocked prefix, matching the existing native
atomic publication helper and ordinary parent-fsync admission contract.

## Freeze and remaining work

R0 source/doc12-file allowlist and hashes are local tooling
`/private/tmp/moosuri-legacy-import-r0-freeze-20261115.json`; they are not runtime
recovery inputs. Root owns independent review, joined Linux, global status and
public acceptance. Those gates remain pending at this record's creation.

No abrupt OS kill/parent-fsync injection, real legacy exporter/schema adapter,
production row comparison, large bundle performance or media/vault import is
claimed. The120second loop deadline is checked between operations and does not
promise a hard OS IO deadline. Backup export success does not replace an independent
latest-journal restore drill or production cutover. Report documents are editable;
retain the original private report/bundle kit separately. Source install state and
operating services are untouched.

## R1 independent review repair

Review found that a final destination or ancestor using the reserved
`.moosuri-restore-stage-*` prefix could be published successfully but ordinary
runtime admission would reject it. The importer now calls the existing
`reject_stage` guard on the resolved destination before any staging, bootstrap or
publication. Ordinary admission remains unchanged. One targeted synthetic test
covers reserved final name and reserved ancestor with a valid empty bundle,
requires `import_invalid_input`, and asserts no data/report/staging output remains.
No native Cargo was run for this repair because the shared lane was allocated
elsewhere; root-owned joined Linux revalidation remains pending.

## Joined Linux acceptance

Integrated on schema50 with the already published Claude/recovery changes. An
independent reviewer reopened the13 frozen files and accepted the reserved-name
repair; root also verified hashes and read the replay/publication and CLI join.
Actual Linux Store3 and CLI1 cases passed, including source/memory/document history,
withdrawal, all-row accounting, reserved destination rejection, fresh-account
isolation, restart and encrypted export. Runtime/parser behavior was not relaxed
to satisfy a fixture. Final Clippy and embedded build passed.
Only public source and synthetic records were sent to the test host.
