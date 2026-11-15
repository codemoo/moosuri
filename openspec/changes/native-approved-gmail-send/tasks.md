# Tasks

- [x] Wire separate native send grant and exact actual scope/identity checks.
- [x] Integrate native transport into existing mail connections/local draft/full preview/final approval and fixed timing.
- [x] Add draft-only capability admission and explicit server-worker exclusion.
- [x] Add schema56 immutable per-revision device effect/payload/execution/commitment contract, with read-session checks unchanged.
- [x] Reuse shared frozen plaintext and Sent equality/observation merge helpers; no automatic resend/failover.
- [x] Add actual synthetic provider+signed-command+Store fixtures for approved bytes/timing/unknown/reconciliation, edit/revoke/dual device, ACK/restart/late receipt, scope/identity rejection.
- [x] Local React typecheck and four focused test files18tests; pinned formatter and diff checks.
- [x] Root serial Rust compiler/runtime outcomes on the integrated schema55→56 source.
- [x] Root generated API-type parity, shared regression checks and independent authority/security review.
- [x] Root acceptance documentation and common support updates; publication checks are mandatory before push.

See [integrated acceptance](../../../docs/reviews/2026-11-15-native-approved-gmail-send.md). No live accounts, provider sends, real credentials, CI or production changes were used. Synthetic approval in fixtures does not authorize any real mail.

R1 repair scope: retire writer-proven uncommitted stale native requests before the32-result limit; filter current connection/binding authority before pending/due queue limits; scope old-request abandonment to its exact revision. Restore migration now applies55 then56 and quarantines native uncommitted invocations while retaining committed recovery/late-receipt identity. Synthetic predecessor fixtures drop later objects before declaring an older captured schema. The32-stale-plus-fresh send and current native/prior55 sealed restore outcomes passed. Root compiler fixes include optional-bool TS annotations, explicit broker command alias, checked SQLite signed conversions/String tuple inference, and a synchronous lexical form-encoding scope so the async provider future remains Send.
