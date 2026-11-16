# Repair-estimate choice, correction and cancellation

`everyday-repair-choice-invalid-correct-cancel` adds one handling outcome to the
public Rust evaluator. Two newly authored synthetic appliance estimates require
the owner to identify the appliance. This covers an invalid answer followed by a
valid receipt-bound choice, retained work through restart, and cancellation of a
resumed task before its held final response publishes.

The owner-visible situation, estimates and turns live in
`moosuri_eval::repair_choice::FIXTURE`. The protocol script sees actual tool
receipts and the real selected owner-input ID; graders and SQLite snapshots never
enter its request. The source facts are independently authored in the grader.
No external shop, appliance service, report, mail or Calendar effect is invoked.

| Stage | Observed user outcome | Maximum model/tool calls |
| --- | --- | --- |
| Observe and ask | Actual search and two reads supply distinct current source-choice candidates and one action-needed notice | 3/3 |
| Ambiguous answer | HTTP rejects `Whichever is cheaper.` as a noncandidate value; wait, original work, notice and receipt state remain identical | 0/0 |
| Correct choice and replay | Owner selects the actual refrigerator card; one answer receipt and exact replay retain the original task/run | 0/0 |
| Reopen/resume | The original run resumes with its selected source and completed reads; next final provider response is held | 1/0 |
| Cancel before publication | HTTP owner cancellation and replay disable the original grant without final publication | 0/0 |
| Late response/reopen | Observe one live supervised worker, release the response, await actual worker retirement with Runtime alive, assert no publication, then reopen and check persistence | 0/0 |

All source ingestion, mission creation, input replies, cancellation and inbox
reads use the authenticated HTTP router. Runtime performs the actual saved reads
and input wait/resume. While the task is waiting, Runtime is shut down before the
valid reply so ACK/replay costs are observed separately; Store accepts the owner
reply and restart resumes the same durable run. The scripted endpoint holds only
the resumed final response, after its dispatch is observed. SQL is read-only
observation, never a mechanism to create success, a wait or cancellation.

The invalid value uses the documented `InvalidInput` HTTP400 contract. Its actual
status/body and before/after observations are retained in evaluator execution
data for grading. The meaningful invariant is exact unchanged persisted state:
no reply row, replacement wait/notice, extra inference or lost completed work.
A valid correction must name a real current candidate from actual source-read
step/call/revision receipts, not a model-invented choice.

The independent grader checks source facts, owner and candidate links, one
correction receipt, original request/model profile/deadline, unchanged tool-slot
identities and counts, one cancel control and retained uncertainty. It separately
checks persisted wait-notice dismissal/suppression and the filtered HTTP inbox.
Cancellation, not task completion, is the successful outcome. This case therefore
remains in planned cases and handling eligibility without inflating task-success
or mission-achievement denominators. Token usage remains actual or unknown.

Required negative controls accept the ambiguous answer, replace its wait, invent
a candidate receipt, repeat a completed read, duplicate a reply, publish a late
candidate, omit live worker settlement, or expose the retired notice. Existing shared stage accounting and
bounded terminal/dispatch waits are reused. Later stages remain not-run after a
failure; no failed case is removed from the denominator.

This is deterministic public protocol and storage evidence. It does not certify
free-form model understanding of arbitrary ambiguous replies, connected-account
clarification, service booking, private report creation or provider delivery.

Run the existing evaluator commands with the configured repository toolchain:

```sh
cargo test --locked -p moosuri-eval --lib
cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json
```

## Local evidence and first failure

Before the live-settlement correction below, the full driver passed all15planned cases and all61required apparatus checks.
The new handling case passed all six stages with actual model/tool counts
`3/3,0/0,0/0,1/0,0/0,0/0`. Its four provider dispatches remain observed while token
usage is `null`: cancellation occurs during the last dispatch, so nominal fixture
usage is not substituted for the missing persisted usage. The local synthetic
report is `/private/tmp/moosuri-repair-choice-final-20261116.json` and is not committed.
The library run passed11/11tests; the final driver also executed the strengthened
mutants against their specific intended failed checks.

The first library run passed all15workflow outcomes but failed the old setup
apparatus control. It assumed `CASES[0]` was completion-eligible; the new first
case deliberately succeeds through cancellation and is handling-only. The
observed control tuple was `(planned1,executed0,pass0,not_run1,completion0)` where
the old expectation required completion1. No product failure was hidden and no
case was removed. The control now uses explicit calculation and handling roles,
asserting both remain planned/not-run and eligible after setup failure. The same
existing denominator test checks both roles without depending on case order.

Integrated acceptance and independent review remain separate from this local
checkpoint. These observations establish the bounded synthetic lifecycle only.

## Cancellation-proof correction

Independent review found that releasing the response and immediately restarting
Runtime could cancel network IO globally. That sequence could mask a broken owner
cancellation fence. The provider ledger records its planned terminal response
before its semaphore hold, so that ledger alone does not prove delivery or worker
retirement. The earlier green report is retained as evidence of this weaker check.

The final stage now requires exactly one supervised run worker before release,
then releases the held response while the same Runtime stays alive. A bounded
watch wait requires actual worker reap and local retirement settlement to reach
zero, with an open watch and `!is_stopping()`. Before shutdown, read-only state and
HTTP inbox observations must still show cancellation without a candidate, output,
effect or visible notice. Restart happens only after that live check succeeds.
A targeted negative control rejects a missing live settlement.

The authorized product delta is a read-only Runtime activity watch and minimal
wiring in three manual Runtime/supervisor test constructors. The watch counts active supervised run workers plus
reaped workers awaiting local settlement; it does not count DB-queued work,
background services or provider requests. A closed channel or stopping Runtime
cannot establish live quiescence. The corrected native evaluator library passed
11/11 tests, including the complete fresh HTTP/SQLite scenario pack and required
mutation controls. This is separate from the earlier driver report; integrated
driver acceptance passes15/15 with62/62 validation checks in the final integrated
source. Frozen independent re-review and public-source Linux acceptance are recorded
in [the integrated report](reviews/2026-11-16-adaptive-continuity.md).

The subsequent agent-test compile exposed two additional manually constructed
Runtime/supervisor test helpers missing the new observer channel/field. The first
compile failed with E0061/E0063 in `subscriptions/missions_tests.rs` and
`tests/connected_missions.rs`; no passing agent-test result is claimed for that
attempt. After the authorized minimal constructor wiring, the agent-test target
compiled and the focused concurrent-shutdown test passed1/1 (205filtered out).
No behavioral test logic changed.
