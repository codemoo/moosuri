## Why
Missing context, conflicting actual sources and duplicate Calendar targets previously ended otherwise usable missions. Repeating the request loses completed work, while fabricating reminder schedules would mix notification delivery with execution authority.

## What Changes
- Extend the original persisted input wait with bounded owner context, receipt-bound source choice and exact existing Calendar target selection.
- Permit context/source clarification on the current read-only recurring occurrence without modifying its template.
- Publish a generic action-needed inbox/WebPush event linked to the original card, using the existing delivery/session/quiet/mute controls.
- Migrate schema69 to70 without replaying authority or inventing schedules; update the restore migration ladder.

## Impact
P4/P5/P6, F29–F37/F42 and R14/R16/R20/R23. Rust Store/agent/server, React mission cards/service worker, SQLite notifications. No new external effect, account scope, future recurring write, actual provider/device acceptance or real-model quality claim.
