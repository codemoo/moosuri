# Evidence-bound proactive suggestions — 2026-11-15

Schema57 adds a fixed suggestion policy and artifact/scheduler/Push/UI integration
on the accepted schema56 native-mail implementation.

One useful workflow: explicitly save an active unmuted followup with no dates,
receive a literal review-date suggestion linked to that exact current owned record,
open the record editor, then independently hide the card or mute the fixed
`followup_review` product topic across different records. No suggestion creates an
agent run, mission, external action or provider call. There is no historical
backfill or general semantic classifier. Pending+visible suggestions are capped at5.

## Local evidence

- `npm --prefix web run typecheck`: passed (main and worker TypeScript).
- `npm --prefix web test -- src/Schedules.test.tsx`: passed19 cases, including two
  new cases for evidence navigation without writes, category mute/unmute and
  independent hide. Existing account/generation/request fencing cases also pass.
- Direct installed stable `rustfmt --edition 2024 --config skip_children=true` on
  changed Rust files: passed. Default `cargo fmt` could not create rustup's temp
  file under the sandbox; the installed formatter was used without downloading.
- `git diff --check`: passed before source freeze.

Integrated Linux acceptance exercised these seven actual Store outcomes:

1. Owned literal evidence→inbox→opaque Push, no execution rows, edit after dispatch
   removes display/link authority and never replays.
2. Stable product topic across distinct records; dated followup alerts remain
   independent; stale preference CAS fails and unmute never revives old Push.
3. Quiet inbox without Push;5-card admission; hide releases capacity; record edits
   do not resurrect an individually hidden suggestion.
4. Held-publication edit fence and transitive parent-evidence withdrawal suppress
   old Push, redact text and remove the current reference.
5. Current-schema quarantine/reopen retains topic mute and requires explicit save.
6. Actual account-management deletion clears both new sidecars and preserves the
   other owner's publication.
7. Real sealed schema56 and57 checkpoints restore forward to57: predecessor
   records are not backfilled; current pending suggestions stay quarantined and
   muted topic preferences survive.

The first six cases passed together. The seventh passed after correcting the
fixture's private restore parent; the current snapshot and reconstructed schema56
predecessor both exercised encrypted checkpoint/restore without loosening capture
or quarantine checks. Existing followup-date outcomes passed5 plus the repaired
predecessor case, source-mute outcomes passed6, and current-schema/native schema55
restore cases passed. Old downgrade fixtures remove new schema57 tables before
asserting an older version; current-schema assertions use the product version.

Independent source review accepted the frozen policy and repaired seventh fixture.
Affected all-target Clippy, the assistant build and React build passed. Generated
Rust API types match the React declarations. Formatting, plan metadata, strict
OpenSpec and source/new-history secret checks run as publication gates.

No production database, credential, provider, real message or real Push endpoint is
used. These are deterministic synthetic lifecycle checks, not evidence of semantic
recommendation quality or mobile notification reception.
