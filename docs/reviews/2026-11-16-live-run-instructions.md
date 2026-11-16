# Live instruction backend checkpoint

Initial source frozen at `920ca20`, directed review repair `0568a9f`, based on
`bf5f850` (preceding reminder continuity and acknowledged mission recovery
included). This worker used synthetic accounts,
local SQLite and loopback streams only; no live account/provider, production,
Push, external send or publication. Independent frozen review and combined
UI/server/schema73 integration remain parent-owned.

## Delivered behavior

POST `/api/v1/runs/{id}/instructions` takes exported `RunInstructionRequest`
`{request_id,expected_attempt,text}`. Text is nonblank, at most2000 UTF-8 bytes and
preserved exactly. `RunInstructionReceipt` returns request/run/thread, original
attempt, per-run sequence, exact text, received/applied/closed and nullable fixed
reason. At most eight receipts exist per run. Exact replay returns the same
current receipt; payload mismatch/new terminal request conflicts. Exact GET
`/api/v1/runs/{id}/instructions/{request_id}` supports uncertain ACK recovery.
No inference is dispatched for ACK. Received POST receipts return202; resolved
POST receipts and exact GET lookups return200.

Ordinary `RunView.supports_instructions` becomes true after the runtime selects a
genuine safe function continuation. Known read-only mission carriers support
queued/planning/executing/verifying guidance. Plain text-only, native credentials,
foreground Mail/Calendar/effect/typed local and artifact-write paths are not
advertised. Original effect and private-report missions are gated out; read-only mission inference
also removes Mail/Calendar preparation/write tool definitions. User text does not
enlarge the frozen grant or approve mail.

Guidance is assigned at a dispatch boundary, stays received while in flight and
becomes applied only after a valid round is committed. Refusal, malformed or
incomplete terminal outcomes close it unapplied. Pending guidance prevents the
previous final, mission plan/input wait or verifier result from being accepted.
The same continuation/run/mission and original budgets continue. Prior tool
results precede user guidance and are reused. Network output cannot insert a user
history item. Old displayed foreground finals become commentary when superseded.

Optional `RunInstructionTarget` resolves original one-off creation receipts to
their actual carrier only; MissionView carries that target and receipts too.
Ordinary conversation APIs still reject internal mission threads. The exact
target run/attempt is used for instruction IO. Applied guidance is projected once
into the effective prior turn's next-turn user history; received/closed guidance
and a new task's authority remain separate. Schema72 also closes restore receipts
and cascades private rows with deletion. Stable mission resume preserves original
receipt identity through a private processing-attempt field; ordinary explicit
Retry does not replay the text.

## Verification evidence

- `cargo test --locked --offline -p moosuri-agent tests::run_instructions:: --
  --nocapture`: **1/1**, 3.42s, containing five actual held HTTP-stream cases:
  ordinary final race, post-tool final race, refusal, malformed response and the
  same mission. ACK does not increase model calls. Revised answers use original
  run/attempt; the completed calculation receipt is retained without another
  calculation. Both invalid outcomes close received guidance. The mission's
  otherwise-valid old candidate is replaced by the revised result under the same
  mission identity and spent dispatch budget.
- Initial runtime compile succeeded. Its test fixture then attempted ordinary
  conversation access to an internal mission thread and received the intended
  `NotFound`; the test now polls actual MissionView, preserving that product
  boundary. Final five-case runtime result above passed.
- `cargo test --locked --offline -p moosuri-store live_instruction --
  --nocapture`: **3/3**, 3.57s, covering exact byte replay, mismatch409 equivalent,
  other owner/new same-owner login, original attempt, UTF-8/count limits, pending
  final barrier, unchanged dispatch accounting, applied-only next-turn history,
  deletion, refusal/restore closure, actual original mission admission mapping
  and cancellation closure.
- `cargo test --locked --offline -p moosuri-store run_instructions::tests:: --
  --nocapture`: **1/1**, 0.00s, schema71→72 transaction rollback, forward creation,
  byte constraint, FK validation and chat cascade of both new tables.
- `cargo test --locked --offline -p moosuri-connectors live_instruction_tests:: --
  --nocapture`: **1/1**, 0.00s, all three protocol user-role capsule round trips and
  provider-output user-item rejection. Runtime covers actual ChatCompletions
  continuation encoding; other live providers were not contacted.
- Rust formatting and `git diff --check` passed. Existing ts-rs serde-attribute
  warnings remain. This worker did not rerun broad suites, server HTTP/build,
  full lint, actual encrypted restore or real-model semantic acceptance; those
  are explicit combined-integration/review responsibilities, not proven here.

The frozen scope is the API/store/runtime path. UI work and its tests live in the
separate live-instruction UI tranche. Further scope additions require integration
review rather than expanding this checkpoint.

## Frozen review repairs

Independent review of `920ca20` identified two material boundaries. The initial
unconditional original-session comparison rejected a fresh login for a durable
mission even though existing durable mission authority intentionally permits the
current same owner and epoch. The support gate also admitted PrivateReport checks,
which can save a document and exceed this first read-only slice.

`0568a9f` keeps original-login matching for foreground/current-session tasks and
allows the currently validated same-owner durable login at the original epoch.
New admission still passes existing mission grant/authority and execution guards;
the submitting session digest is audit data. It excludes requested private
reports alongside original Mail/Calendar effects, and aligns POST received202 /
resolved200 without changing the receipt or GET contract.

- `cargo test --locked --offline -p moosuri-store live_instruction --
  --nocapture`: **5/5**, 6.19s. The additional regression logs out the originating
  durable session and admits/exactly replays from a fresh same-owner login,
  verifies its actual audit digest, retains current-session rejection/original
  acceptance, rejects a changed epoch and rejects private-report instructions.
  ACK dispatch count stays zero. Prior ownership, replay, limits, final/history,
  delete/restore, admission mapping and schema tests also pass.
- Formatting and `git diff --check` pass. The runtime/connector protocol body did
  not change in this repair; no broad repeated suite was run. Root owns combined
  server/full lint/build and held real-model/browser verification.
- Root integration additionally identified that the standalone initial source
  omitted schema72 from `cutover::restore::migrate`. Root owns inserting the real
  `0072_run_instructions.sql` before monitor schema73 in that registry; this must
  be retained in the integrated source. The worker's direct quarantine/schema
  tests did not prove the actual encrypted-restore route, as stated above.

Combined integration and later repairs are recorded in [live guidance and monitors](2026-11-16-live-guidance-monitors.md); that record supersedes the parent-pending notes above.
