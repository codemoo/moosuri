# Owner missions design

A mission owns one hidden chat/run carrier, an immutable request and versioned owner steering.
`UserContext` is retained only for actual API/control admission. `chat::execution_authority` dispatches
to a mission-specific account/epoch/grant check; session-only adapters cannot consume that principal.
External permits carry either a real optional session or a closed public-HTTP owner path. There is no
synthetic session, second provider transport, browser impersonation or unrestricted tool runner.

The existing supervisor gives one mission an execution slot while retaining the other for chat.
The mission driver stores a bounded model plan and reconstructs fresh model messages from that plan,
original acceptance and typed completed slots. A completed proposal is also saved as explicit mission response data in the same writer transaction as its model receipt; restart consumes that record rather than repeating an acknowledged planning/final request. Provider continuations are not mission authority.
Each model request uses the existing six-call ledger and dispatch nonce, tool results the twelve-slot
ledger. A one-day admission deadline is immutable across every attempt. Repair cycles are persisted,
max2, shared for tool failures and verifier failures. At most five execution model rounds follow planning.

A stable local checkpoint can acquire a new attempt after pause/restart. Completed local slots retain
results and do not execute again. A committed unknown model/HTTP dispatch is terminal uncertainty and
is never automatically replayed. Pause/cancel atomically fences state before future dispatch; late usage
settles through existing non-content receipts. Steering cannot mutate the original request/checks,
pinned tools/provider or budgets. Explicit current-session resume binds the genuine current session.

The deterministic verifier resolves `(step,call_id)` only in this carrier's completed typed tool ledger.
Wrong/missing/error receipts cannot prove a check. Exact calculation output and source revision/quote
are compared with actual results; output-contains proves shape only. User-review checks never auto-pass.
Model prose has no authority to declare achieved. A missing result, failed result, usable partial result,
and committed unknown outcome remain distinct. Revocation clears derived private candidate text.
Control idempotency returns current readable state, preventing old response snapshots bypassing withdrawal.

Deletion appends a content-free mission ID to the existing destructive journal and removes payloads,
controls/revisions and carrier. The minimal UUID/digest tombstone prevents recreation under the old
request UUID. Restore disables every mission grant and quarantines unfinished rows; historical results
remain readable unless their provenance is withdrawn. Global restore invalidation is not misinterpreted
as a historical source withdrawal by the mission result projection.
