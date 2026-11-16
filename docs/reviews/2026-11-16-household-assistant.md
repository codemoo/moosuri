# Household assistant scenarios and shared execution — 2026-11-16

This tranche extends the [everyday assistant evaluation](2026-11-16-everyday-assistant.md)
with fresh household requests, a full saved-source follow-up lifecycle and common
mission/intent handling. It belongs to P0 evaluation, P2 conversation, P3 memory,
P4 execution and P5 follow-up. It does not close all of those plan stages.

## Fresh actual-model development evaluation

Inputs and expected outcomes were frozen before dispatch against public baseline
`0bf25532be04dde77f4dccf8d945716e15ca0187`, using GPT-6.1 Terra/high through
the isolated official-CLI evaluation bridge. The expected-outcome SHA256 was
`7c3246ab4e0be5f5cecb11d49d8c59597d07e56fa6d294c3fb0ad0d296c4ab5e`.
Only synthetic owners, pets, sources and amounts were used. Credentials, SQLite
files and raw model traces are outside Git. This bridge is evaluation apparatus,
not a new production subscription adapter.

The first run passed **12 of 13 requests**. The failing request stays in that
original denominator; development reruns are separate.

| Scenario | First result and independent evidence |
| --- | --- |
| Remember Moka's salmon feed and sibling's Tori/chicken feed | Two separate owned saved memories; one model call each |
| Correct Moka to duck; recall both pets | Correct revision, names and owner relationships; one and three calls |
| Forget only Moka; recall from a new conversation | Moka/history removed, Tori retained; two and three calls. A second owner's same-topic beef memory is unchanged |
| Ask for tomorrow 07:00, then answer “우산 챙기기” | Question uses zero inference; answer fails after one call, saves no reminder and incorrectly says reminders are unavailable |
| Ask for tomorrow 08:00, then “됐어, 취소해 줘.” | No reminder created and draft closed: a functional pass, with one unnecessary model call on cancellation |
| Read return status, replace the source, then read again | Actual revision 1 pending and revision 2 refunded/23,800 KRW reads; three calls each, no stale-answer reuse |
| Read subscription fees and save a cost report | Actual 14,900−2,400=12,500 calculation receipt and one persisted document/version. Admission one call plus four mission calls; partial/user-review state retained |

Document existence/version and final owner state were independently read from
SQLite after shutdown. Run/mission receipts establish real source reads and
calculation. These requests are a small development set, not a blind holdout,
statistical latency comparison or general assistant quality score. Missing token
usage remains missing; model counts do not establish a measured token percentage.

## Product repairs and consolidation

- Reminder replies use a bounded activity-label grammar rather than a fixed verb
  list. Polite natural cancellation closes only the immediately preceding owned
  draft. Both use the existing transaction/replay receipt and no inference.
  Original date/timezone, expiry, session/sequence and account boundaries remain.
  Literal reply parsing is separate from time filling and persistence.
- Independent review found a nonleading conflicting clock: “우산은 내일 오후
  8시에 챙기기” could be saved under the prior 07:00 draft. Civil-time cues now
  check word boundaries throughout a bare label; an actual Store regression
  requires no reminder. Explicit `내용:` remains literal message text.
- [Committed mission rounds](2026-11-16-mission-rounds.md) share one fresh/recovery
  handler for guidance, plans, saved/pending tools, waits and verification.
  Invalid final text/legacy plans settle instead of leaving a run active;
  malformed guided recovery cannot silently skip its assigned guidance.
- Current-model Calendar, artifact and memory vocabularies derive from shared
  typed definitions ([계약과 검증](2026-11-16-intent-contracts.md)). Current proposals require the advertised keys and supported
  codes. Persisted legacy decisions retain replay compatibility. Calendar missing
  duration can continue from a concrete follow-up rather than a freeform reason
  the continuation cannot interpret.

No new permission dialog or authority is added. Existing final mail approval,
user isolation, revocation, saved effect receipts and local review remain.

## Reusable follow-up harness

The [saved-source return/refund scenario](../followup-scenarios.md) exercises the
actual authenticated router, Runtime and SQLite: parent read/proposal → owner
accept/replay → unchanged skip → revised-source child/read/verification/notice →
no duplicate → stop/replay → reopen with the notice suppressed and policy revoked.

Public fixtures are separate from the independent saved-state grader. Ten
negative controls tamper with child identity, source scope, read evidence, notice
suppression, policy state, task/schedule/occurrence links and contradictory facts.
Independent review found that matching notice IDs and positive keywords alone
could falsely pass. Visible and saved task links now agree, and this explicitly
requested quotation is graded against independent complete expected quotes
(whitespace normalization only), including source, candidate and displayed output. Due times use disclosed public Store clock methods;
this is synthetic scheduling, not real elapsed-time or remote mailbox monitoring.
Unchanged/stop/reopen stages have zero model/tool calls. Source replacement can
remove an earlier parent tool row, so shared stage accounting counts new
`(run_id, step, call_id)` identities rather than subtracting total row counts.

## Acceptance

Native integration passed: Store reminder/filter26, evaluator library11, all14
planned evaluator workflows and all53 apparatus checks, scoped core/Store/agent/
evaluator Clippy with `--no-deps --all-targets -- -D warnings`, and CLI/evaluator
build. Delegate parser/mission/continuation/effect checks and their overlaps are
recorded in the linked scoped reviews; they are not summed as unique tests.

The original reminder sequence and pending-draft cancellation were repeated in an
isolated copy of the original synthetic world, with new conversations and the
reviewed binary. All4 turns completed, with model dispatches `[0,0,0,0]`. Exactly
one “우산 챙기기” reminder retained the original next-day07:00 Asia/Seoul time;
then cancelling another draft left it unchanged. Individual local HTTP/terminal
observations were13/11/12/12ms. These are bounded development observations, not a
statistical latency claim. The original13-request12/13 result remains unchanged.

Independent reviews covered reminder time/ownership boundaries, fresh/recovered
mission settlement, current intent versus persisted replay, and evaluator oracle
independence. The identified reminder conflict and both oracle gaps were fixed
and re-reviewed without unresolved material findings in the reviewed scopes.

Exact public-source Linux acceptance and publication are the remaining gates.
This checkpoint is not an operational deployment or stable-release claim.
