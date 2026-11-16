# Saved-source follow-up evaluation

`everyday-return-status-followup-stop` is one public synthetic lifecycle in the
Rust evaluator. A newly authored kettle return progresses from warehouse receipt
and pending inspection to approved refund with settlement still pending. These
are saved-source claims, not proof of an external refund or payment.

Reusable agent-visible inputs live in `moosuri_eval::followup::FIXTURE`, separate
from `grader/followup.rs`. The scripted compatible provider derives source reads
from actual owner acceptance pins and final facts from actual tool receipts; it
receives neither SQLite observations nor grader answers.

| Stage | User outcome and observed boundary | Model/tool limit |
| --- | --- | --- |
| Read status and propose | Actual mission reads source revision 1, passes quote verification and creates a scoped monitor proposal | 2/1 |
| Accept and exact replay | HTTP owner acceptance creates one policy/schedule; replay returns the same active receipt | 0/0 |
| Unchanged | A due occurrence records `monitor_no_new_evidence` with no child, notification or inference | 0/0 |
| Changed source | HTTP ingestion updates the same source to revision 2; one child reads that revision, verifies it and publishes one local notice | 2/1 |
| Unchanged again | The changed fingerprint cannot create a duplicate child or notice | 0/0 |
| Stop and replay | HTTP stop revokes the reusable policy and schedule, retires the notice; replay returns the same result | 0/0 |
| Reopen | Reopened SQLite state preserves stop and notice suppression; another due tick cannot admit a child | 0/0 |

Owner operations, source ingestion, admission, monitor lists and inbox reads use
the real authenticated router. Both mission executions use the real Runtime and
SQLite receipt/verification path. The product has no HTTP scheduler-tick API;
the driver explicitly calls public Store `enqueue_scheduled_reminders_at` and
`poll_recurring_missions_at`, then wakes Runtime. Due times advance monotonically
within the original one-hour policy expiry and are recorded in the report row’s `synthetic_clock` metadata and graded independently. This is synthetic scheduling evidence, not a wall-clock latency or
live scheduler benchmark. No product DB mutation forces admission, success,
publication or suppression.

Read-only SQLite observation distinguishes a hidden inbox item from its saved
notification row. After stop, the row must retain its identity and immutable
source provenance while `dismissed` and `push_suppressed` are set. Policy and
schedule must both remain revoked. Source scope must contain exactly the chosen
source, excluding another independently saved source. Parent and child receipts
must link the actual owner, source revisions and candidate evidence. The visible
inbox item's `mission_id`, `schedule_id` and `occurrence_id` must match the actual
changed child and its persisted recurring occurrence. The read-only observer also
joins the saved notification through that exact occurrence and notice identity;
a correct notice ID with a wrong task link fails.

This deterministic task explicitly requests the current status as a literal quote.
The grader therefore requires the complete independently authored expected quote
for both source revisions in the source receipt, saved candidate and output,
normalizing whitespace only. It does not import fixture constants to build gold.
Extra commentary, a contradictory paid claim or negated approval fails the literal
quote contract. This narrow oracle is not a semantic grader for arbitrary summaries
or paraphrases, and the quoted approval still does not prove payment settlement.

Required tampered-observation controls duplicate the child, widen source scope,
remove the changed-source read, expose the retired notification, and reactivate
the persisted policy. Additional required controls keep the correct notice ID but
change each visible task/schedule/occurrence link, or retain the correct quote and
append a contradictory paid/negated-approval claim. Setup/workflow/stage failures remain in the original
planned denominator. Entered stages fail until their postconditions and actual
model/tool budgets pass; unentered stages remain `not_run`. Usage comes from
actual model receipts and retains missingness. Internal mission server timings
remain explicitly missing where no RunView timing was sampled.

Run locally with the configured repository toolchain:

```sh
cargo test --locked -p moosuri-eval --lib
cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json
```

This extends the existing report schema without another format change. It is a
public deterministic regression, not live-model semantics, a connected-thread
provider exercise, a blind holdout or permission to send mail. No production,
legacy service, credential, collector, cron or CI is touched.

## Local acceptance and observed repairs

Native acceptance on the original `d8d6d0e` freeze passed all 11 library tests and
all 14 planned full-driver cases. All 48 required apparatus checks passed. The
follow-up row observed four model dispatches with 52 input and 20 output tokens
from actual scripted-provider receipts. Stage model/tool counts were `2/1, 0/0,
0/0, 2/1, 0/0, 0/0, 0/0`; all seven stages passed. The local synthetic report is
`/private/tmp/moosuri-followup-final.json` and is intentionally not committed.

The initial fixture correctly failed when the driver supplied an extra nonempty
semantic constraint: the product returned partial with “Additional constraints
need owner review” and created no monitor proposal. The driver now uses the
original owner task and exact source acceptance pin without adding that redundant
constraint. Product review semantics and the success oracle were unchanged.

The lifecycle also exposed a stage accounting defect: revision ingestion removes
the prior source-derived parent tool slot. Subtracting total slot counts therefore
reported zero tools while the changed child had one real read. Shared stage
accounting now compares persisted `(run_id, step, call_id)` identity sets. The
final report correctly observes one child read despite removal of the parent's
old slot. Entered-stage failures still retain observed counts and later stages
remain not-run.


## Independent review corrections

Review of the original freeze identified two grader P2 gaps despite its passing
report. A notice ID could pass with an incorrect user-facing mission link, and
positive status keywords could pass a correct quote followed by an unsupported
“already paid” assertion or denial of approval. Those original false-positive
conditions are retained as required negative controls. The correction observes
persisted occurrence/schedule/mission links and enforces the explicit normalized
literal-quote contract described above; it changes no product behavior or fixture
answers. Targeted rustfmt and diff checks pass. Cargo/native acceptance of this
review correction remains pending root integration; the original 48-check result
must not be presented as verification of these additional controls.

## Integrated correction acceptance

The reviewed correction passed root-owned native integration:11/11 evaluator
library tests,14/14 planned workflows and53/53 required apparatus checks. The
follow-up lifecycle still observed stage model/tool counts `2/1,0/0,0/0,2/1,0/0,
0/0,0/0`. Independent re-review confirmed the task-link and contradictory-quote
findings were addressed. See the [integrated record](reviews/2026-11-16-household-assistant.md).
