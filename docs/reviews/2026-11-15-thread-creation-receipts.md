# Owned thread creation receipts — 2026-11-15

`POST /api/v1/threads` now accepts an optional UUID `request_id`. A keyed request
uses its canonical UUID as the thread ID and stores a SHA256 digest of the
original trimmed, validated title in an owner-scoped schema71 receipt. Concurrent
or repeated same-owner requests return the same current thread summary; a renamed
summary does not change the original request digest. A different original title
returns409. A legacy thread with the same ID is a collision, never adopted as a
creation receipt. Requests without a key retain the previous creation behavior.

Current owner/session authorization is checked inside the creation transaction,
before looking up a receipt. Another owner may independently use the same UUID.
The receipt has an account foreign key and no thread foreign key. It stores no
raw title. Account deletion's existing tenant-table purge also covers these rows.

Deletion returns404 on later replay of the creation request. The independent
DeleteThread journal handler upserts a minimal NULL-digest tombstone even when
neither the target thread nor a creation receipt exists. This matters when a
snapshot predates creation but the latest independent journal includes deletion:
the stable thread/request UUID lets replay fence the old request. Merely keeping
random thread IDs and receipts in the restored DB would allow resurrection.
Both Store opening and historical restore migrate before replaying this handler.
No new GET endpoint, inference retry or effect/approval change is introduced.

## Native evidence

All accounts, titles and state below are synthetic. No production or real-account
access, live model call, push, deployment or CI enablement occurred.

- Store thread-creation suite: **4 passed**. Checks concurrent same-key canonical
  titles and one row, current renamed summary, changed-title conflict, invalid
  UUID, legacy-ID collision, cross-owner isolation, logout, delete/reopen and
  latest deletion replay over a snapshot captured before creation. Snapshot
  replay is exercised at schema71 and through an actual70-to71 upgrade.
- Focused HTTP creation test: **1 passed**. Observes concurrent201 responses with
  the same summary,409 mismatch,400 invalid UUID, deletion followed by404 replay,
  unchanged legacy requests and401 after logout.
- Store migration filter: **13 passed**, including direct offline restore70-to71
  migration with account foreign-key enforcement and the existing encrypted
  predecessor-restore/lineage scenario.
- The exact previously failing schema37 learned-thread deletion regression:
  **1 passed** after the fixture repair described below.
- Selected Store/server all-target Clippy with `--no-deps -- -D warnings` and
  workspace formatting passed. Clippy ran before the final fixture-only repair;
  final migration tests compile and execute the repaired fixture strings.

## Historical fixture repair

Twenty-four existing synthetic downgrade fixture files needed removal of the new
receipt table before rewinding their schema version. A dynamic predecessor
restore fixture additionally needed the same removal. The fixed current-schema
assertion in the mission-input fixture now expects71. Production schema creation
remains strict, with no `IF NOT EXISTS` compatibility workaround.

The broader migration filter initially exposed old omissions in seven migration
tests, and the root Linux execution run exposed an eighth related case. A
frozen schema70 checkout reproduced the same media migration failure before
schema71 existed: surviving native-model-mail tables still referenced the dropped
credential-broker jobs table. Only the encountered fixtures were repaired with
missing post-target table, trigger and column removals. Their original rollback,
retry, preserved-data and deletion assertions remain. The migration filter and
schema37 deletion regression then passed. Other unrelated downgrade fixtures
were only updated to drop the new schema71 table.

Final Linux creation 4, HTTP 1, migration filter 9, mission-upgrade 1 and
execution 44 checks passed, as did affected-crate Clippy and the CLI build.
Native account deletion explicitly purged keyed receipts while preserving the
other owner’s receipt; all seven destructive-journal checks passed. Independent
combined frontend/backend review found no material defects. Real Chrome lost
creation receipt → reload → explicit recovery retained the same thread/message
IDs and one completed run. See the [integrated review](2026-11-15-assistant-core-recovery.md). This note makes no live-readiness or production latency
claim. The shared mutation-receipt OpenSpec is updated with the frontend change.
