## Decisions
Reuse the existing five-second fixed reminder loop, time oracle, owner/epoch
checks, quiet hours, inbox and Push lifecycle. A sidecar maps internal once
schedules to an artifact revision/date and a stable first schedule source ID.
Internal schedules are absent from normal reminder lists/controls/budgets.
Explicit saves alone arm future dates; migration does not backfill. Equal dates
merge, future snooze collapses earlier dates, offline catch-up takes the latest
elapsed date. Delivered same-instant history prevents clock-rewind replay.

Artifact invalidation retires previous pending claims and permanently suppresses
old Push before publishing the new head in the same transaction. Old private
inbox/schedule/occurrence snapshots are redacted. Dispatched outcomes remain
factual, while display/retry authority is fenced. Source preferences resolve the
stable record source, covering new dates/revisions. Restore pauses all schedules
and quarantines Push; explicit resave can re-arm only future dates.

## Limits
The existing oracle ends at2100-01-01 UTC; later record dates remain briefing-only.
No semantic date inference, periodic nagging, model execution, proactive suggestion
policy, actual-device delivery guarantee, external mail/send or Push recall.
Historical sidecars use existing artifact identity/version bounds; no GC added.
