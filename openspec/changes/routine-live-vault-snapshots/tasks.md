## Implementation

- [x] Add same-writer immutable DB+journal capture without sealing source.
- [x] Add explicit opt-in local periodic worker/service flags and graceful drain.
- [x] Reuse exact age framing/validation with distinct snapshot purpose/magic.
- [x] Add explicit point-in-time restore using full quarantine/new identity rules.
- [x] Add narrow actual DB/HTTP/worker/recovery and malformed-stage checks.
- [x] Document older-snapshot gaps and unchanged sealed-current contract.
- [x] Main independent frozen source review and integrated Linux acceptance.
- [ ] Independent newer encrypted journal transport/pinning/historical forward recovery.
- [ ] Production deployment, real client and off-host/hardware durability acceptance.
