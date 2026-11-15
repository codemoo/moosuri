# Tasks

- [x] Add exact opaque selected identity and separate local body grant without altering legacy signed bytes.
- [x] Wire archive/live vault companion selection, fixed provider body read, bounded MIME/plaintext and durable signed settlement.
- [x] Admit only owner's successful preview selectors through Store/API; fence original expiry/device authority and hide revoked late results.
- [x] Provide literal settings/original-chat selection/result UI with automatic reuse and lost-ACK identity preservation.
- [x] Add synthetic native/Store/API/React boundary fixtures.
- [x] Format and diff checks; React typecheck and two focused test files (7 tests) passed locally.
- [x] Root Linux native Google/Store/Runtime/HTTP/default-off outcomes and final affected all-target Clippy/assistant+companion build: [acceptance](../../../docs/reviews/2026-11-15-native-google-body.md).
- [x] Independent frozen security/authority review accepted; separately reviewed root UI account/session-lifetime repair accepted.

Real-account OAuth/provider/quota, signed release and production rollout are outside this implementation unit. CI remains disabled. No live credentials or account data were accessed.

Additional local regression evidence: broad `Chat.test.tsx` ran22/24 passing with two existing failures (reader scroll-position assertion; private-file fixture querying an old visible library label). Both exact failures reproduced against an isolated unmodified `cb25283f` web archive; no unrelated chat repair was included. Two focused native-credentials/body files passed7/7. Typecheck passed. Pinned1.89 formatter check and `git diff --check` passed. Rust execution is root-owned and still pending.
