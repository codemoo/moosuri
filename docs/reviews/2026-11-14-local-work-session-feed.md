# Local work-session producer — 2026-11-14

F41 now has a once-only Rust `moosuri work-sessions codex` command with mandatory
user-selected directory, exact HTTPS origin, expected custom connection UUID,
private token file and durable state path. It uses the existing custom ingest
protocol, not a new server, automatic service/cron, model or local tool executor.
No assistant schema, external dependency or default source scan was added.

The first supported format is the narrow canonical public Codex JSONL subset
verified at openai/codex b1573f102413e96a6481087922bf425231222dee. The
[command documentation](../local-work-sessions.md) links the actual writer,
envelope, SessionMeta/EventMsg, TurnAttribution and TUI human-submission sources.
No private session dataset was read to infer compatibility. Claude, exec,
subagents, inherited/forked context, missing attribution and unknown client
formats remain unsupported. Evidence is selected literal prose user requests;
assistant/tool/config/attachment/response-item records never become text evidence.
Credential/context/code/dump-looking requests are omitted, with incomplete
coverage kept honest. A finished marker means a turn ended, not work success.

The producer checks owner permissions, no-follow regular-file identity,
device/inode/size/nanosecond mtime/full SHA256 stability and bounded complete lines.
It handles recent-active versus idle-partial turns, incomplete trailing lines,
finished appends, semantic turn hashes and inode replacements without inventing
remote deletion. A rotating bounded scan avoids starving later files. It persists
run intent and exact batch UUID/body/cursor before dispatch, then advances local
file/object position after original receipt/finish or the R1 exact committed-position
reconciliation described below. Token
values are not stored or printed; pending selected prose is0600 private state and
is cleared on completion. Destination/scope/auth/position mismatches stop.

## Initial R0 verification

**Four focused connector outcome tests passed** using real synthetic local JSONL
files, private state/lock files and an actual loopback HTTP client/server. The HTTP
server is an in-memory **mock of the custom-ingest protocol**, not the actual
moosuri Store or source-ingest router. This evidence does not establish Store,
account, real client export, real HTTPS endpoint or production collection acceptance.
No private session, provider/model/mail/Push or legacy service/DB/cron was touched.

```sh
cargo test --locked -p moosuri-connectors --lib work_sessions::tests -- --test-threads=1
```

The four outcomes cover:

- Reject-before-commit, commit/lost-batch-ACK and lost-finish-ACK, retaining the exact
  request/body/cursor across file changes; one committed turn, no replacement UUID
  or extra run, no token/config/path/attachment/tool/assistant canaries in envelope
  or durable state.
- Mixed human/automation/subagent/context/secret-looking records, explicit prose
  selection and honest unsupported coverage; partial→finished update on the same
  turn and hash/inode replacement deduplication.
- Recent active turn and unterminated last JSON line, then idle partial evidence
  and a later complete append without importing partial-line text.
- Wrong expected connection, denied status, redirect without following its trap,
  state within source/Git, symlinked token and special-file token refusal, with no
  batch or cross-origin credential dispatch.

Connector all-target Clippy with `-D warnings` passed. Full CLI/server/agent build
passed; `moosuri work-sessions codex --help` confirmed every source/destination/
private-path input is required and no `--data-dir` is required. Final formatting,
strict OpenSpec and whitespace gates passed.

CLI all-target Clippy on this worker's older base encounters an unrelated Store
`large_enum_variant` warning in `ExternalToolPreparation`; the root integration
already fixed its parcel-origin size. This worker does not change that Store code
or claim that older-base broad lint passed.

## Remaining boundary

The custom `/runs` endpoint has no client idempotency key. A lost run-start ACK
retains intent and fails closed; it does not adopt another active run. Automatic
verified run-start reconciliation is a next connected contract. Batch/finish ACK
recovery is implemented and tested as above. Actual Store/HTTP restart recovery is covered by R1 below; Linux and independent
acceptance are recorded in the [joined integration evidence](2026-11-14-recall-skills-producer-integration.md).
Actual selected client/account use remains separate. Full history coverage, other formats and cursor GC
remain outside this first bounded unit.

## R1 independent-review corrections

The initial four-test mock evidence missed a production finish conflict: actual
Store startup fences an unfinished run as unknown even when its batch committed.
R1 now checks the original receipt plus the current exact after-sequence and
checkpoint and rejects another active run. With no active run, it advances only
that committed local cursor without sending finish or rewriting unknown history.
If the original run is still active, finish is attempted and current position/run
state is rechecked. Position/checkpoint/active-run disagreement retains pending state.

One composed regression uses synthetic local input and the **actual Store and
production HTTP router**: commit one batch, fault only the finish response,
shutdown/reopen the Store, observe the real409 finish conflict/unknown run, then
recover the same original receipt/cursor. It asserts one actual batch/source
receipt, no additional ingest/finish on reconciliation, sequence/checkpoint1 and
unchanged unknown historical run. This is one production-boundary result, separate
from the mock protocol cases; real client data, account/HTTPS deployment and
broader integration remain unverified.

R1 also applies the established root/current-UID plus nonmutable-nonsticky ancestor
rule before source/token/state admission. Trusted writable sticky ancestors require
the next owned component. One focused fixture proves that private0700/0600 final
paths under a mutable nonsticky ancestor are rejected, then accepted after fixing
the ancestor. Existing no-follow/descriptor/ownership checks remain.

R1 results: connector outcomes **5 passed**, including changed-position/checkpoint/
conflicting-active-run refusal and the unsafe-ancestor fixture; actual Store/router
restart regression **1 passed**. Commands:

```sh
cargo test --locked -p moosuri-connectors --lib work_sessions::tests -- --test-threads=1
cargo test --locked -p moosuri --features test-fixtures --bin moosuri work_sessions::recovery_tests::actual_store_router_restart -- --test-threads=1
```

Final formatting, strict OpenSpec and whitespace checks passed. Original R0
manifest/evidence are retained, not overwritten. No broad suites or unchanged CLI
rebuild were run for this correction.

## Superseding start recovery

The later [schema48 acceptance](2026-11-15-custom-ingest-start-recovery.md) adds
optional keyed start intent and exact ACK recovery. This original dated evidence
remains historical; pre48 unkeyed uncertain starts retain the limitation above.
