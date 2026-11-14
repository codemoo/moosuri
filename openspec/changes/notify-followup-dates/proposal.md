## Why
F28/F32 save due/review/snooze fields but do not notify the owner. Connect explicit
saved dates to the existing fixed reminder inbox/Push path with exact receipts and
withdrawal fences, without idle model calls or a new automation framework.

## What Changes
- Future date once reminders, equal-date/snooze/offline coalescing and no history backfill.
- Pinned artifact revision/date receipts and current owned record links.
- Atomic artifact mutation/provenance fences and stable source mute across edits.
- Schema0046 upgrade/restore support and synthetic Store-to-inbox-to-Push evidence.

## Impact
Artifact persistence, scheduler, notification source/Push eligibility and React
artifact/inbox views. No external provider call or new permission screen.
