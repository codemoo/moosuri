## Context

The published provider/account foundation supplies current UserContext and verified
revision/model/effort choices. This unit connects them to durable plain-text chat.
The full architecture/effect/recovery contracts remain accepted future work.

## State and identity

One tenant/request UUID has one digest(thread,text), one saved user turn and one run.
Duplicate submission returns that run before resolving current defaults; a changed
payload conflicts. Retry changes the attempt on the same run, keeps the original
provider/model/effort pin and never copies the question into the composer. Only the
latest thread turn may be retried until conversation branching is supported.

Queued and running are active; completed, failed, cancelled, incomplete, refused
and outcome_unknown are distinct terminal states. A new request may not dispatch
on a disabled or changed provider revision. Personal settings changes affect new
runs only. Original account/session authority is checked at claim/checkpoint.

Claims are single-writer transactions. A private opaque lease binds tenant/run,
attempt, unpredictable dispatch nonce and original authority. Dispatch intent and
minimal usage receipt commit before HTTP. Startup changes dispatched/running work
to outcome_unknown; it never automatically requeues it. Queued work was not sent
and can run if its original authority remains valid.

## Limits and context

Two model calls run globally, one per user. Admission permits one active run per
thread, eight queued per user and64 globally; FIFO eligible users avoid monopolies.
Inputs are at most8,192 UTF-8 bytes; output is bounded to256 KiB/128 assistant items,
8 KiB per append,4,096 requested output tokens and180 seconds. Explicit retries are
bounded to32 attempts. These limits do not imply accurate monetary cost or billing
caps; full configurable budget policies remain part of P1.

Canonical messages retain commentary/final/unknown phase. The next request takes a
bounded suffix of successful conversation turns plus the new user message, preserving
whole turns as a contiguous suffix; an oversized turn stops selection instead of
reviving older turns past it. Failed partial output is not asserted as successful history. Unsupported
context is explicit rather than silently replaying arbitrary tools/reasoning.

## Streaming and deletion

Committed changes produce per-thread monotonic events in the same transaction.
Keep the latest512 events, replay at most64 per read, and reset to a fresh snapshot
if a cursor is no longer retained. History pages contain20 chronological turns and
a same-snapshot event cursor. Delta/phase events avoid resending the full growing
answer. React batches rendering without artificially delaying tokens.

Authenticated SSE checks current account/thread authority before each bounded batch.
Expected account headers and request-lifetime fencing apply to every callback.
Delete removes private turns/output/events and fences late workers. Cancellation
immediately stops further visible output; already accepted remote work may continue.
Closed browser streams do not cancel the independent runtime.

## Accounting

Each attempt has one minimal call receipt. Usage snapshots replace/coalesce counters,
never add the same cumulative snapshot twice. Unknown counters remain unknown;
reported totals are labeled as reported. Deleting a conversation removes text, while
minimal usage observations can still settle through the original opaque lease.
No timeout/cancellation is claimed to be a refund. Administrator usage lists contain
account metadata/counters only, not personal prompts or replies.

## Validation

File-backed storage tests and actual loopback model transports cover duplicate/lost
ACK, attempt fences, restart unknown/no replay, cancelled/deleted/revoked workers,
phase and event cursor behavior, fair admission and accounting. React behavior and
an actual Chromium/Rust journey cover composer continuity, smooth delivery, reconnect,
retry without overwriting a draft, mobile scroll and account switching. Tests and
independent review precede each public commit; live model quality remains separate.
