## Implementation
- [x] Reproduce overlapping retry/send, obsolete earlier-turn retry, unavailable Cancel after streamed retry and blocked hung deletion.
- [x] Fence same-thread mutations and settle run-action transports on canonical target transitions with immutable owner checks.
- [x] Bound deletion receipts and GET-only uncertainty recovery without automatic write replay.
- [x] Preserve latest inventory and fence stale pre-deletion snapshots and dead-slot callbacks.
- [x] Reproduce the captured-inventory regression against the base App.tsx and retain new failure/recovery observations.

## Verification
- [x] Six affected frontend suites passed 111 tests, and app/worker TypeScript checks passed.
- [x] Production web build, strict named OpenSpec validation and diff check.
- [x] Record final evidence and limits in the dedicated review note.

## Independent deletion-owner follow-up
- [x] Reproduce all later deletion controls remaining disabled after streamed authoritative target absence.
- [x] Abort/release only the matching deletion owner and fence its late response/finally from newer deletion/reconciliation.
- [x] Seven focused deletion observations, app/worker typecheck, strict named OpenSpec validation and diff check passed.
