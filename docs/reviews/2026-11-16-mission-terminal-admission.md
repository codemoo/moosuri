# Mission terminal admission and ordinary continuation routing

Base `7fa6b73`; synthetic loopback providers/accounts only. No live model,
production service, external mail, push or deployment was used.

## Material defects

The mission driver accepted any cleanly parsed model response without checking
its outcome. A loopback response returned a correct decimal receipt followed by
valid final JSON with `finish_reason=length`. The actual Runtime reported the
mission as `achieved`; the new regression failed before the fix. Candidate JSON
and owner-input question JSON shared the same unchecked path, including report
verification. The driver now rejects incomplete/refused rounds before recording
an executable mission capsule. Prior successful receipts and observed usage stay;
the original dispatch budget and restart rules do not reset.

The generic Korean `이어`/`계속해` cue also sent explicit answer/explanation
continuations to the task classifier. The cost gate now bypasses that extra round
for recognized complete generic forms. The same shared core helper prevents an
immediately preceding task receipt from inferring a selected task for those forms.
Other date/delegation/control cues remain independent;
bare continuations, arbitrary named task controls and selected tasks retain the
existing interpretation path. No task authority comes from this heuristic.

Independent review identified the upgrade boundary: older acknowledged
`mission_round` capsules had no outcome proof and could resume before the new
HTTP gate. New capsules persist an allowed terminal marker atomically. Recovery
checks it before plan/action admission, unfinished tool replay or candidate/input
parsing. Unmarked legacy capsules become unknown without new inference or tool
work. The local finalization capacity exception requires a completed marker too;
no original budget or authority fence is reset. Already completed receipts remain.

## Evidence

- Before the terminal fix, the actual Runtime test failed with
  `length/candidate: achieved`, despite the incomplete provider outcome.
- Frozen source `695a002`: `cargo test --locked --offline -p moosuri-agent
  tests::missions:: -- --nocapture` passed **10/10**. The new terminal test covers
  nine streams: successful result and incomplete/refused candidates, granted
  private reports, questions and tool bytes, preserving prior completed work and
  observed dispatch accounting through reopen. A separate three-case test reads
  actual HTTP outcomes, commits the pre-fix capsule format, loses the process
  before use and proves upgrade cannot replay a candidate, question or pending
  tool. Existing report/save, scheduled execution and acknowledged-recovery cases
  also pass.
- `cargo test --locked --offline -p moosuri-agent tests::mission_chat:: --
  --nocapture` passed **6/6**. Immediately after saving a real recurring task,
  both ordinary continuations use exactly one answer call and do not change that
  task. Named controls including `고객 답변 조사 이어줘`, bare continuations,
  selected tasks and dated continuations remain eligible. Original-request clock
  clarification/reopen and original target controls continue to pass.
- `cargo test --locked --offline -p moosuri-store acknowledged_final --
  --nocapture` passed **4/4**: acknowledged final recovery at the six-model,
  twelve-tool and attempt32 boundaries, plus original expiry settlement.
- `git diff --check` and formatting of affected Rust files passed. The parent
  owns pinned OpenSpec validation, integrated lint/Linux checks and browser/live
  model evaluation; no results from those pending runs are claimed here.

## Bounds

This checks actual transport parsing, supervision and durable mission outcomes;
it does not measure language quality, production latency or provider acceptance.
The audit inspected original-input continuation, restart, recurring dispatch and
suppression paths but makes no claim that all assistant defects were found.

Parent integration, final independent review and actual browser/live evidence are
recorded in the [combined acceptance](2026-11-16-assistant-main.md).
