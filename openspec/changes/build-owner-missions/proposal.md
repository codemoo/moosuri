# Change: bounded owner-admitted one-off missions

## Why
F27 needs an actual persistent plan/read/verify result flow with explicit background authority,
recovery, truthful uncertainty and bounded repair. Ordinary chat/session continuation is insufficient.

## What changes
- Add explicit authenticated mission form/API and an immutable original acceptance snapshot.
- Use a distinct SQLite owner grant for durable execution; retain real session checks for foreground mode.
- Reuse the model transport, run leases, typed tool ledger and HTTP service for a closed read subset.
- Verify actual current-run receipts before achieved; keep semantic review explicitly partial.
- Persist pause/resume/steering CAS, original budgets, revocation, deletion tombstones and restore quarantine.

## Impact
F27 bounded one-off subset. Schema25 is reserved after push23/vault24; this isolated schema22
workspace applies it as version23 for validation. No migration of those moving features is included.
Recurring missions, conversational capture, changes-only comparison and broader tools remain pending.
