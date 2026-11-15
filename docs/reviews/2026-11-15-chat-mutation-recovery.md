# Foreground chat mutation recovery

Synthetic frontend work on base `7d73cbb`, with creation-identity follow-up from `dd07158`. The initial frontend checks did not execute Rust; final backend/browser integration is recorded in the [main recovery review](2026-11-15-assistant-core-recovery.md). No real-user data, live provider or production service was used.

## Defects and resulting behavior

A stalled message POST previously kept its pending request and transport lock forever. The focused UI reproduction failed before the change because no explicit same-request recovery appeared after 30 seconds. New-thread creation and run cancel/retry shared the same unbounded JSON path.

These main-chat callers now explicitly request one 30-second complete-response deadline. Expiration means the admission receipt is unknown, and invokes authenticated GET reconciliation without automatically replaying writes. An uncertain message keeps its original request UUID, text, selected attachments and contexts; explicit same-ID resubmission leaves the next draft alone. Caller/account abort settles promptly even when the transport ignores cancellation, and owned or late bodies are cancelled.

Canonical SSE/history admission clears its exact pending identity before aborting a redundant message HTTP receipt. This releases a hung send immediately and permits the next draft once the admitted run completes. The HTTP abort cannot recreate the cleared pending request.

A timed-out new-thread creation retains a separate immutable creation UUID/title and the original immutable message beside its next draft. It refreshes inventory without automatic creation or message replay. Both explicit Retry new conversation and composer retry reuse the original creation identity; a button-only recovery never sends its pending message. The existing bounded authenticated sessionStorage lease retains these carriers through reload under the exact account/public-session scope. Successful creation atomically migrates the original pending message and separate next draft to its confirmed thread before clearing the carrier. A late creation receipt does not select away from another thread the user chose while waiting. If the admitted target itself already has another draft or unresolved message, a local collision fence preserves both original slots and request identities, makes no message POST and leaves same-identity recovery available after that target is finished.

The corresponding parent-owned schema 71 contract accepts optional POST /threads request_id: same account/UUID/original canonical title returns the same thread, changed title conflicts 409, and an admitted deleted thread remains not_found 404 without recreation. Old clients omitting the key retain their behavior. The frontend clears definitively unusable creation carriers; composer-first definitive failure restores its unsent text without another ID or automatic send. Backend implementation/tests are recorded separately in the [creation receipt review](2026-11-15-thread-creation-receipts.md).

Run cancel/retry deadlines refresh canonical history and release the action lock; expected_attempt remains unchanged. Unrelated mutations, thread deletion, binary uploads and long-running administrative probes retain their existing timing.

## Verification

- Before the fix: the focused hanging-send UI reproduction failed with no same-request resubmission button after 30 seconds.
- After the fix: seven focused frontend suites passed, 126 tests: `npm test -- --run src/Chat.test.tsx src/stream-responsiveness.test.ts src/App.test.tsx src/chat-drafts.test.ts src/Media.test.tsx src/PrivateFiles.test.tsx src/chat-state.test.ts`.
- Before the creation-identity follow-up: the focused explicit empty-conversation recovery test failed because the original request_id was undefined.
- New observations cover POST header/body stalls, caller/account abort despite an abort-ignoring transport, late body cancellation, no automatic replay, same-ID/payload explicit recovery and next-draft continuity, canonical SSE release of a stuck send, admitted cancel/retry with lost receipts, and creation inventory reconciliation retaining immutable creation/message requests beside the next draft. Follow-up observations cover reload without automatic replay, explicit same-creation/same-message recovery through both controls, button-only recovery without sending, definitive 404/409 cleanup, exact-session carrier recovery/rejection late receipt/thread-switch continuity and preserving both draft slots during a recovered-target collision.
- Existing observations continue to cover attachment-ID/revision resubmission, account switching/logout, draft recovery, stream reconnect, queued/running actions, history paging and immutable contexts.
- `npm run build` passed TypeScript app/worker checks and Vite production output.
- Pinned OpenSpec 2.0.0 strict named validation of `bounded-chat-mutation-receipts` passed with telemetry disabled.
- `git diff --check` passed.

## Limits

Browser timer suspension can postpone deadlines; visibility/focus/online recovery remains. This is controlled transport/UI evidence, not a real-network or model-quality claim. Integrated real-Chrome recovery passed with the embedded Rust backend and a synthetic provider; schema71/API/Linux evidence is in the main review. Production deployment was not performed.
