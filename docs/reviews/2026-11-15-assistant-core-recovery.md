# Main assistant recovery and completion — 2026-11-15–16

The review followed message admission, personal model readiness, tool completion,
durable answer classification and browser recovery. Synthetic inputs and isolated
databases were used; no real mail, production account or deployed service changed.

## Fixed behavior

- A stalled message HTTP receipt no longer holds the composer after canonical
  SSE/history proves admission. Foreground mutations have a complete-response
  deadline; unknown results retain their exact original identities and payloads.
- Thread creation has an owner-scoped idempotency receipt. Explicit recovery
  after timeout/reload reuses the original creation UUID and, when requested,
  original message UUID. Late receipts and target-slot collisions preserve
  independent drafts. Deletion tombstones prevent replay from resurrecting a
  thread, including after restoring a snapshot predating its creation.
- Personal foreground rounds preserve their validated HTTP pool while checking
  current authority and token readiness each round. No model/effort downgrade or
  inference retry was added.
- Abandoned undispatched refresh acquisitions release their exact nonce even if
  cancellation occurs before the SQLite actor delivers the acquisition result.
  Previously a 45-second lease could defeat the next 35-second readiness attempt.
  Cleanup also allows the last Store to close when its mailbox is saturated.
- Commentary such as “I will check” alone no longer counts as a completed answer.
  The actual final item metadata governs completion, including late phase labels.
- A completed native Gmail read using tool slot 12 can still produce its final
  answer with remaining model capacity. Tool slot 13, exhausted model dispatches,
  expired deadlines and revoked authority remain disallowed.

## Evidence

The commentary-only case failed with the former completion rule, then passed
five actual streamed variants after the fix. The twelfth-tool case likewise
failed before the fix and passed through persisted native result/resume, with
original counters/deadline preserved and a negative exhausted-model case.

Linux integration passed 10 native-mail continuation cases, 40 personal
subscription cases and 29 transport cases. The first execution suite passed
43/44; its failure was an old synthetic schema-downgrade fixture omitting newer
tables. That fixture was corrected without weakening production migrations.
Final schema71 Linux integration passed creation store 4, HTTP 1, migration
filter 9, mission-input upgrade 1 and execution 44/44. Affected-crate strict
Clippy and the embedded Linux CLI build passed. Native account deletion with
keyed receipts passed, preserving the other account’s receipt; all seven
destructive-journal cases also passed.

The new lost-acquisition-ack test passed both cancellation boundaries on Linux.
The saturated-mailbox shutdown test passed with the cleanup task unpolled and
already waiting. Independent review found the first fallback could retain a
strong sender across suspension; the revised weak-only waiting path and both
shutdown boundaries passed re-review.

Frontend checks passed 126 tests across seven affected suites plus the production
build. Mutation deadlines, immutable payload recovery, reload, both creation
controls, separate drafts, late selection and target-slot collisions are covered.

Independent source review accepted terminal classification, native continuation
capacity, personal authority/transport, revised refresh cleanup and the combined
frontend/schema71 creation, deletion and restore contract.

## Real browser integration

A temporary Chrome profile used the freshly built embedded application and a
loopback synthetic provider. A real message POST reached the backend while its
HTTP response was deliberately held. SSE/history acknowledged the request and
the client aborted the redundant HTTP wait. Another message was admitted before
the 30-second deadline; no duplicate initial request or run was created. One
follow-up wording entered a planner unsupported by the greeting-only fixture and
returned a protocol failure; a supported greeting then completed. This fixture
limitation is not counted as model-quality acceptance.

A second case accepted thread creation in the backend then reset its browser
response. The original pending message and separate next draft survived reload,
with one stored thread and zero runs before explicit recovery. Recovery submitted
the same creation UUID/title and received the same thread ID, then sent exactly
one message with its original UUID/text. The backend contained one completed run
and one model dispatch; the separate next draft remained in the composer.

Publication uses workspace formatting, plan/OpenSpec validation and source/history
secret scans. CI stays disabled; no operating deployment is part of this change.

See [foreground mutations](2026-11-15-chat-mutation-recovery.md),
[personal transport](2026-11-15-personal-foreground-transport.md) and
[creation receipts](2026-11-15-thread-creation-receipts.md) for implementation
bounds. These checks do not establish production latency or universal task
quality, and do not constitute a production deployment.
