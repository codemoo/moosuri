## Implementation

- [x] Add same-writer immutable DB+journal capture without sealing source.
- [x] Add explicit opt-in local periodic worker/service flags and graceful drain.
- [x] Reuse exact age framing/validation with distinct snapshot purpose/magic.
- [x] Add explicit point-in-time restore using full quarantine/new identity rules.
- [x] Add narrow actual DB/HTTP/worker/recovery and malformed-stage checks.
- [x] Document older-snapshot gaps and unchanged sealed-current contract.
- [x] Main independent frozen source review and integrated Linux acceptance.
- [x] Bounded independent newer encrypted journal transport/pinning/historical forward recovery: [receiver acceptance](../../../docs/reviews/2026-11-15-vault-journal-publication.md). Periodic publication retains a recovery gap; zero-loss off-host deployment is not claimed.
- [ ] Production deployment, real client and off-host/hardware durability acceptance.
