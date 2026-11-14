# Followup date notifications — 2026-11-14

F28/F32 now connect explicitly saved followup due/review/snooze dates to the existing
fixed reminder worker, inbox, quiet hours and opt-in Push. There is no idle model
call, periodic nagging or external mail/send authority. Schema0046 adds the pinned
artifact/date mapping and stable source identity; ordinary reminder lists, chat
controls and the64 reminder admission limit exclude these internal schedules.

Each distinct future due/review date notifies once; equal instants merge. Future
snooze produces one notice and collapses earlier dates, retaining later dates.
Past dates alone and pre-migration history never backfill. Explicit future snooze
can re-arm an overdue record. Offline catch-up takes only the latest elapsed date
per saved revision. Existing delivered same-instant history also prevents replay
under clock reversal. Dates after2100-01-01 UTC remain saved and briefing-only.

Artifact edits, lifecycle changes, mute, snooze, delete and recursive provenance
withdrawals use the same mutation transaction to retire old reminder claims,
redact/dismiss stale snapshot text and permanently latch old notice Push
suppression. Queued/claimed Push loses authority; dispatched actual outcomes
remain factual while passive display/retries are denied. Source mute follows the
record across later dates/edits and unmute never replays old notices. The inbox
shows the actual artifact revision/date and only a current owned record link.
The record editor shows actual server-saved reminder receipts independently of
unsaved dates. Sealed restore retains mappings/history while pausing schedules
and quarantining Push; explicit current-owner resave re-arms future dates only.

## Verification

Synthetic native Store suite: **18 passed**, using the actual Store persistence,
existing fake-clock scheduling APIs, inbox and Push enrollment/queue/commit/display
contracts. No Push transport, real provider, model, mailbox, old service or
production DB was called. Selected filter:

```sh
cargo test --locked -p moosuri-store --lib -- schedules::followups artifacts::tests source_mute_latches actual_current_schema_restores --test-threads=1
```

The six new outcome tests cover truthful revision/date links and owner isolation;
held reminder + dispatched display fences; past-date/no-backfill policy and
explicit snooze; offline coalescing and claim/reopen/delivery duplicate fences;
resolved/cancelled/muted/snoozed/deleted/recursive-withdrawn records against
unscanned/queued/claimed Push; stable source mute across new versions and future
dates, unmute without replay; quiet inbox without Push; equal-date merge; and
pre-followup upgrade without history backfill until an explicit save. Existing
artifact and adversarial generic source-mute tests remain green. The existing
actual sealed-current restore fixture now includes followup receipts, paused
state, no automatic publish, reopen and explicit owner resave/delivery.

Affected React suite: **24 passed** across `Artifacts.test.tsx` and
`Schedules.test.tsx`, including saved-versus-unsaved date receipts and exact
artifact navigation with pinned revision/date. Existing request/CAS/account and
source mute/quiet/inbox behavior remains covered in those files.

Affected core/Store all-target Clippy with `-D warnings`, the full CLI build and
canonical generated type export, main/worker TypeScript checks and Vite production
build passed. Formatting, strict OpenSpec and diff whitespace checks also passed.
Old-schema fixtures
mechanically drop the new sidecar before reconstructing their older snapshots.
Independent review and root integration are separate acceptance steps.

## Limits

No actual browser Push delivery, real device/permission/OS lifecycle or server
uptime guarantee was exercised. The existing five-second polling/quiet/Push
semantics remain the transport boundary. Already displayed device notifications
cannot be recalled. Semantic date inference, broad topic/suggestion policy,
recurring followup nags, autonomous model work and history GC remain unimplemented.
