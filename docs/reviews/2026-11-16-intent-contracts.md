# Current-intent schema contracts — 2026-11-16

## Findings and scope

Calendar exposed unconstrained operation/clarification strings even though Store
supports only create/update/delete and pending continuations need machine field
codes. Arbitrary clarification prose was accepted at record admission, then produced
a generic prompt without an answerable pending field. Its parser also accepted
omitted Option keys despite the advertised required object. Artifact had the same
required-null omission gap and unknown decision/non-null no-command shapes fell
through to a generic clarification. Memory already enforced complete keys and
finite decisions; its schema literals duplicated the parser's enum vocabulary.

Shared core vocabulary now drives schemas/parsing/admission. Calendar reasons are
time/date/duration/target/account/ambiguous/unsupported; a new clarify requires one.
Unknown, blank, missing or prose values are rejected. No string is normalized into
a missing-field code, civil value, target or effect grant. A small shared agent
helper checks complete object keys and builds nullable enums only.

## Compatibility and effects

Strict finite-reason and complete-key checks apply to new Calendar record admission
and current-model Calendar/artifact parsing. Existing persisted Calendar DTO defaults,
legacy freeform/null reason replay and generic message fallback are retained; no
stored decision is rewritten. New unsupported Calendar decisions keep nullable
reason compatibility. Existing owner/account/auth/profile, exact literal quote,
source, civil-time, target, original-grant and publication checks stay authoritative.
No new approval prompts or effect classes were introduced; mail still requires its
explicit final-preview approval. Schema/version and production services are unchanged.

## Verification

On the isolated checkout based on `0bf2553`, locked/offline native filters passed:

- Agent `calendar_model_reason_contract`:1; actual production parser→Store checkpoint
  rejects prose/blank/null clarify reasons and every missing key, then preserves the
  original pending duration draft and applies one counted typed write after its answer.
- Agent `artifact_model_contract` and `calendar_current_schema`:1 each; explicit nulls,
  finite schema codes, complete keys, duplicate rejection and legacy DTO decoding.
- Agent `memory_intent::tests`:9; original literal quotes, temporal/action shapes,
  duplicate/non-command rejection and Unicode/byte bounds remain unchanged.
- Agent `artifact_tools::tests`:6; includes malformed classifier rejection before a
  second call/write, grounded create/edit receipts, editor race, untrusted source,
  withdrawal, external evidence and reopen behavior. The all-dialect schema case passes.
- Agent `request_continuation`:4; original Calendar answer and duration preservation,
  withdrawal/stale ETag fences and existing weekly mission continuation.
- Agent `sealed_calendar`:3; create/update/delete and personal sealed retry/unknown
  receipt checks. `all_day_missing_last_day`:1; original civil span is retained.
- Agent `runtime_artifact_no_command_then_calendar`:1; strict no-command followed by
  the separately acknowledged Calendar classifier still writes once with two calls.
- Store `legacy_persisted_clarification`:1 and
  `request_continuation_reopen_freezes_operation`:1; replay fallback and reopened
  original operation/field/answerless-completion fences remain intact.

The first artifact sweep exposed three old fixture prompts naming compound
read/compare→report work while their server expected `artifact_intent`. The old
composition prompt also mixed reading a stored document with English event creation;
its unchanged composite cue selected `mission_chat_decision`. Those four fixture
prompts now name direct document persistence or non-reading document context for
the intended Calendar composition. All original read/write/receipt/withdrawal/reopen
assertions remain; the repaired filters passed. Product routing was not changed.

Direct formatting/diff and strict pinned OpenSpec validation passed. Targeted
`cargo clippy --locked --offline --no-deps -p moosuri-core -p moosuri-store -p
moosuri-agent --all-targets -- -D warnings` passed; existing ts-rs serde-attribute
notices remain non-failing. Independent frozen source review found no material defect in the current-output
versus persisted-replay boundary, continuation admission or preserved fixture
assertions. Root-owned integration results are recorded in
[household acceptance](2026-11-16-household-assistant.md).

No live model/provider calls, private data, legacy services, cron or CI were used.
See [OpenSpec package](../../openspec/changes/shared-intent-contracts/).
