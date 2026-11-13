## 1. Implementation
- [x] 1.1 Add owner-private stopped checkpoint with fsynced fail-closed source seal and age outer encryption.
- [x] 1.2 Verify bounded fixed framing, journal/DB identity, semantic CAS/history/receipts/reservations/tombstones.
- [x] 1.3 Restore into private staged new identity, clear sessions, durably abort pending operations and atomically publish without overwrite.
- [x] 1.4 Document exact current-only trust, copied-checkpoint fork limits, interruption/retry and source retirement.
- [x] 1.5 Reject other-UID-mutable nonsticky/foreign-owned ancestors consistently and reserve staging destination names.

## 2. Acceptance
- [x] 2.1 Pass focused native storage/recovery/HTTP and Clippy checks on frozen source.
- [x] 2.2 Independent storage/security review and Linux acceptance by integration owner.
