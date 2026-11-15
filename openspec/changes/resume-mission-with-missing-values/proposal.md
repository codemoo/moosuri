## Why

A bounded mission can prepare supported mail and Calendar actions but a missing required value currently ends the original task. Asking for that value must release worker capacity and preserve completed work and original authority.

## What Changes

- Persist a typed missing-value question on the original mission card and accept an idempotent owner reply through the existing authenticated API.
- Resume the same run with original acceptance, receipts, profiles, counters and deadline.
- Limit values to one missing mail recipient or Calendar date, start clock and duration for an already admitted one-off action. Values grant no operation authority.
- Reuse original action journals and exact final mail approval.

## Capabilities

### New Capabilities
- `mission-input-continuation`: Finite typed owner value continuation.

### Modified Capabilities

None.

## Impact

R14/R16/R20; F24–F27/F42/F63; existing additional-capabilities-plan B/D. Rust DTO/store/runtime/API, React mission card, schema69 normal-open and restore quarantine. Synthetic fixtures only; no actual provider or production acceptance.
