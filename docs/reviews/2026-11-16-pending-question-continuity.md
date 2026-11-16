# Pending-question continuity — 2026-11-16

Baseline: `c0029f2390d8306791a5516316604737537b2680`.
Only isolated synthetic Store/model-protocol/typed-IO fixtures are used. No live
provider, credential, production/legacy service or data, cron, CI or settings change.

## Evidence and resulting behavior

The first unchanged outcome case failed on the baseline after a Calendar duration
question, `30 or 45 minutes`, reopen and `45 minutes`: the correction no longer had
its original pending context. No write had occurred. Admission inherited context
with pending=0 while the accepted `ambiguous` reason did not save a pending field.

The fix retains only the already established field that the typed partial draft
still lacks, including a `time` question whose complete local clock lacks an
admitted timezone. It preserves complete values, literal answers and original expiry.
Display asks the saved duration/time/target question while the acknowledged reason
and old replay payload remain unchanged. Existing scope validation still blocks
field/operation replacement and answerless completion.

A complete generic reply such as `취소` or `됐어, 취소해 줘` now closes only the
immediate owned question before a new classifier plan is created. Request replay
and reopen reuse the local completion; no saved mission, reminder or event is
cancelled. A separate later complete request follows ordinary current-user
admission. The shared eligibility lookup retains session/epoch/model/profile,
expiry and invalidation gates. Reminder drafts reuse the generic reply grammar
while keeping `cancel reminder` local to their own route.

Composite cancel-and-new-action messages, named task/event cancellation, quoted
or conditional text are outside the local cancellation grammar. Civil-date/clock
interpretation is unchanged; this is bounded question continuity, not general
correction of every conflicting historical time expression or unlimited dialogue.
No new approval prompt, action grant or mail final-approval bypass is introduced.

## Verification checkpoint

The baseline lost-context failure is recorded above. The fixed same-case fixture
reached its intended single 45-minute write; an initial test incorrectly called
the finished lease again expecting success, and was corrected to require its
existing Conflict fence and unchanged write count. Cancellation/replay/reopen
preservation and stopped synthetic expiry rejection passed their first native run.
The final unchanged ambiguity case passes in the native `request_continuation`
filter, including one typed write, original title/date/start and corrected end,
reopen continuity, and rejection of the finished lease without a repeated write.

Frozen source/tests: `fa0539de0542e5bddca826df2fd22a825f6834e5`.
Locked/offline native commands on the isolated Rust1.89 environment passed:

- Agent `request_continuation`:5; original same-request Calendar/mission answers,
  withdrawal and stale ETag fences plus the new ambiguity/reopen/correction case.
- Agent `pending_question_cancel_replay`:1, exercising both whole Korean replies;
  saved mission/reminder/event states remain unchanged, exact replay/reopen is
  local, and a separate fresh request has its own title/date/clock and write.
- Agent `ambiguous_pending_question_expiry`:1; a bare answer cannot reuse expired
  original event authority after reopen.
- Store `only_whole_generic_question`:1, `legacy_persisted_clarification`:1,
  `literal_activity_labels_and_cancel`:1 and
  `household_draft_reply_reopen_and_cancel`:1; quoted/conditional/composite/named
  requests stay outside generic cancellation and reminder behavior is retained.
- `cargo clippy --locked --offline --no-deps -p moosuri-agent -p moosuri-store
  --all-targets -- -D warnings`:passed. This is scoped lint, not a workspace claim.
- Direct targeted rustfmt and `git diff --check`:passed.

Filters overlap and are not added into an assistant-quality score. The initial independent review found the timezone gap documented below.
Final strict integrated OpenSpec validation and root-owned publication acceptance
are recorded in [the integrated report](2026-11-16-adaptive-continuity.md). No live semantic/provider acceptance is
claimed by these fixtures.

The expiry fixture changes only the stopped synthetic pending-context expiry to
represent elapsed time; it injects no action/result/success row. Other outcomes
use original parser→Store→counted typed IO and public owner operations.

## Timezone review follow-up

Independent source review of `fdc2cb0` found that `still_missing(time)` checked
Calendar start or mission time presence but omitted the frozen unresolved
timezone. A complete clock could therefore lose its pending `time` question after
an accepted ambiguous timezone reply. The bounded fix uses the existing literal
IANA validator with original frozen profile/plan metadata; no zone is guessed.
Relative/interval mission proposals retain their existing lack of a zone requirement.

The locked/offline native agent filter
`missing_zone_survives_ambiguity_and_reopen` passed both new outcomes: complete
Calendar clocks and one local mission clock, no initial timezone, `my usual
timezone`, reopen, then explicit `America/New_York`. A changed browser
`Asia/Seoul` remains excluded. Calendar performs one counted original-title/date/
clock write with 15:00–16:00 UTC boundaries; the mission saves one original-task
Once schedule at independently fixed 2027-03-01 14:15 UTC. Exact mission request
replay and completed-lease fences do not duplicate saved work. The test count is
2 passed,209 filtered; direct rustfmt and `git diff --check` also passed.

These are scripted Store/typed-IO outcomes, not live-model or provider acceptance.
Recovery from conflicting or unsupported literal timezone expressions in earlier
answers remains constrained by the existing full-lineage validator; this change
does not broaden that correction grammar. Frozen independent re-review of `08fb7d2` found no remaining material issue.
Root-owned integrated native and public-source Linux results are recorded in
[the integrated report](2026-11-16-adaptive-continuity.md).
