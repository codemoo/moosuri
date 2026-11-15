## Implementation
- [x] Bound connecting and silent raw-byte reads; preserve abort fencing and release timers.
- [x] Reset the consecutive failure budget on validated progress or delayed comment heartbeat.
- [x] Retain live fresh-send receipts, cursor and paginated history; retain duplicate/missing-receipt reconciliation.
- [x] Add controlled transport, recovery, POST/SSE race and logout observations.

## Verification
- [x] Relevant React suites (65 tests), focused final recovery check, production web build and strict named OpenSpec validation.
- [x] Record verified scope and limitations in docs/status.md and the review note.

## Snapshot recovery follow-up
- [x] Bound read-only GET fetch and JSON body to one 30-second deadline; keep mutation timing and dispatch unchanged.
- [x] Cancel owned/late bodies, distinguish ordinary deadline failure from account AbortError, and preserve lifetime fencing.
- [x] Add GET connect/body/close, unchanged POST timing and canonical snapshot loading/refresh observations.
- [x] Six relevant React suites passed (71 tests); TypeScript checks, strict named OpenSpec validation and diff check passed.
