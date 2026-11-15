## Implementation
- [x] Reproduce a hanging send with no recovery affordance before the fix.
- [x] Add explicit complete-response deadlines for main-chat mutations; retain unrelated timing.
- [x] Preserve immutable uncertain sends and release redundant HTTP transport on canonical admission.
- [x] Reconcile run actions and new-thread inventory without automatically replaying writes.
- [x] Cover abort/late body, explicit same-ID recovery, next-draft continuity, SSE admission and new-thread failure.

## Creation identity follow-up
- [x] Reproduce the missing creation UUID on explicit empty-conversation recovery.
- [x] Persist bounded, exact-session creation UUID/title carriers; preserve immutable pending messages and next drafts through reload.
- [x] Share explicit creation recovery between the empty button and composer, with atomic slot migration and selection fences.
- [x] Cover explicit recovery through both controls, no automatic sends, definitive 404/409 cleanup late receipt/thread-switch continuity and target-slot collision preservation.
- [x] Integrate and verify the parent-owned schema 71/server creation receipt and retained deletion contract.

## Verification
- [x] Seven focused frontend suites passed 126 tests.
- [x] Production web build and strict named OpenSpec validation.
- [x] Record final checks and limits in the review note and docs/chat.md.
