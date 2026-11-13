## 1. Implementation
- [x] 1.1 Add separate Rust ciphertext schema1 service, immutable CAS operations and fsynced fail-closed journal; storage/restart/cancel/nonce tests.
- [x] 1.2 Add single-use session-bound handoff, online authenticated adapter and current-epoch fencing; authority and actual loopback HTTP tests.
- [x] 1.3 Add actual Rust wasm32 module Worker, bounded CSPRNG RPC and owned pending core adapter; native state test and browser entropy/generation/rollover checks.
- [x] 1.4 Add React setup/unlock/CRUD/TOTP/CSV/key changes; real Chrome setup/CRUD/lost-ACK/rotation/import isolation checks.

- [x] 1.5 Repair independently reproduced stale-key Worker admission and concurrent distinct-item receipt reconciliation; native exact-head tests and real two-window/async/lost-ACK evidence.

## 2. Acceptance
- [x] 2.1 Root independent crypto/service/client review, two finding repairs and schema24 integration.
- [x] 2.2 Targeted Linux and actual Chrome integration gates.
- [ ] 2.5 Separate-UID/proxy deployment and real mobile gates before a release claim.
- [ ] 2.3 Complete client archive, standalone vault backup/restore, old-v1 migration and signed-client distribution.
- [ ] 2.4 Complete account deletion/outbox and trusted-device credential broker as separate increments.
