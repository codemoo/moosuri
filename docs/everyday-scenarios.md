# Everyday continuity scenarios

The public Rust evaluator runs three synthetic multi-stage workflows through the
real authenticated HTTP router, Runtime and fresh SQLite store. They extend the
existing contract pack. They do not measure live-model language understanding,
blind holdout quality, production reliability or baseline non-inferiority.

Owner-visible inputs are reusable in `moosuri_eval::everyday::FIXTURES` and
`tools/moosuri-eval/src/everyday.rs`. They contain situations, source text and
owner messages only. They have no evaluator expectations. Scripted provider logic
lives in `model/everyday.rs`; independent checks live in `grader/everyday.rs`.
Future live-model drivers can use those inputs without importing the grader.
All names, amounts, reminders and sources were newly authored synthetic data.

| Case | Owner-visible flow | Observable completion | Per-stage model/tool limits |
| --- | --- | --- | --- |
| `everyday-quote-inflight-guidance` | Read saved quote and calculate; owner adds a shipping-detail instruction while the last dispatch is held; replay the instruction; reopen | Same run/attempt, one applied receipt, shipping and total facts, each original read/calculation receipt exactly once, persisted result | read+calculate 4/3; ACK+replay 0/0; continuation 1/0; reopen 0/0 |
| `everyday-quote-owner-clarification` | Read saved quote; ask delivery criterion; reopen while waiting; owner answers; resume and replay the answer | Original run and acceptance survive; no repeated read, one answer, one calculation; partial result explicitly retains semantic owner review | read+ask 4/2; waiting+reopen 0/0; answer+resume 2/1; reply replay 0/0 |
| `everyday-reminder-receipt-change-cancel` | Create tomorrow's reminder; move the just-created reminder; replay change; cancel; reopen with an unrelated reminder present | Exact identity/revision chain, 10:00→10:30 on the same local day, one replay run, revoked target and unchanged decoy, no target notification | every stage 0/0 |

## Observation and failure semantics

`runner/everyday.rs` owns shared stage accounting, bounded polling and continuity
drivers. The ordinary chat driver shares terminal observation with the in-flight
scenario. Format version 3 preserves the existing report fields and adds `stages`
to every row. Stage records include status, limits, actual dispatch and saved-tool
counts, elapsed time, and named postconditions. Zero-call ACK, passive wait,
replay and reopen stages cannot silently consume another model/tool call.
Tool counts refer to actual SQLite tool slots, not prose claims.

All three cases remain in the primary denominator. A setup failure is `not_run`;
a workflow failure is `fail`, with later stages `not_run`. Stage limits or failed
postconditions fail the case even when its final outcome grader passes. Token
usage still comes from actual model receipts and retains missing values. A local
reminder uses no model receipt; the absence of a receipt is not invented usage.

Each case is independently graded from saved observations. Required negative
controls remove the applied instruction receipt, duplicate a completed tool,
remove the preserved read, falsely claim achievement after free-form context,
redirect cancellation to the decoy, and replace the exact replay run identity.
No final prose sentence is an oracle. Numeric facts, receipt references,
owned identities, acceptance, revisions and result states determine outcomes.
Reminder grading observes the saved SQLite rule/revision/state before and after
change, exact replay and reopen. Independent epoch arithmetic applies UTC+09:00
for this modern Asia/Seoul fixture: tomorrow at 10:00 becomes 10:30 on the same
local day, exactly 1,800 seconds later. The API list must contain only the decoy;
the revoked target must remain a persisted SQLite tombstone. Required time and
tombstone mutants reject a one-minute error or resurrected target.

The monitor change/no-duplicate/stop flow is deliberately outside this pack:
these three completed executable workflows keep this change bounded. Existing
monitor-specific product acceptance remains separate.

Run with the repository's configured Rust toolchain:

```sh
cargo test --locked -p moosuri-eval --lib
cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json
```

Reports are synthetic local artifacts; do not commit runtime databases, model
traces, credentials or personal fixtures. CI remains disabled. The OpenSpec
change records the integrated acceptance and remains available for review.

Integrated native acceptance passed all13 public workflows, every planned
continuity stage and the independent tampered-observation controls. The three new
flows use5/5/0 model dispatches respectively; instruction ACK/replay, passive wait,
reopen and reminder controls have zero model calls. Waiting/criterion work retains
its truthful partial state and original evidence.

Intermediate failures are preserved in the [integrated review](reviews/2026-11-16-everyday-assistant.md).
They exposed stale scripted mission/cancellation assumptions and reminder
observations that incorrectly expected local-time receipt text and revoked rows
in the active list. Observation now reads the persisted UTC rule/tombstone and
independently checks the Seoul civil day. Extra negative controls reject an
incorrect shift or resurrected target. Separate actual-model acceptance is
documented in the review; it does not turn these scripts into semantic testing.
