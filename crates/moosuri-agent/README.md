# Supervised knowledge runtime

`Runtime::start(Store)` owns a supervisor and at most two model tasks, independent
of HTTP and SSE clients. Store claims enforce one active call per user, queue
eligibility, pinned provider revisions and durable dispatch receipts. The runtime
polls queued work every 250 ms and accepts explicit `wake()` notifications.

`stop()` fences new claims and cancels local model work. `shutdown().await` drains
children within four seconds and bounds the supervisor join to five seconds.
Dropping the final handle also cancels supervision: no task owns that handle.
Interrupted dispatched work is recorded as `outcome_unknown`; unrecoverable
checkpoint failures are settled by the supervisor when storage recovers; persistent
failures remain dispatched for startup recovery, never automatic replay.

Text/refusal and late phase changes checkpoint through opaque leases. Output is
bounded to 256 KiB/128 items, UTF-8 appends to 8 KiB, batches to 16 patches with a
32 ms flush interval, and each model call requests 4,096 output tokens. Authority
checks run every 250 ms while streaming; every private store write rechecks the
lease. Usage snapshots coalesce separately and may settle after cancellation or
deletion through the original receipt. No refund or remote cancellation is implied.

Canonical Responses phase is preserved. Other protocols omit commentary only in
the wire projection and strip final phase metadata without changing stored history.
Unknown phase is not inferred to be final. Verified tool-capable models can search/read
account-scoped sources and explicit memories. Explicit conversational remember/correct/forget
requests use the separate sealed memory-command path. Bundled task procedures are read on
demand through `skill_read`; `decimal_calculate` provides checked decimal-string sums,
differences and products. External actions and attachments remain unavailable.
Exact reported model identity and a successful transport terminal are
required for completion. There is no automatic retry, model change or fallback.

`model_client` provides the same validated hosted/local constructor to the runtime
and admin provider probes. It performs no network IO and never logs credentials.

Synthetic file-backed DB and actual loopback HTTP tests cover partial visibility,
disconnected subscribers, duplicate ACK dispatch, streamed errors, refusal/limit/
identity states, cancel/delete/logout fences, shutdown/drop, reopen without replay,
UTF-8 patch bounds, timeout/truncation/connection failure, transient store pressure,
exited-worker settlement and wire/late phase behavior. These are implementation tests,
not live-model quality or full agent evaluations.

The [knowledge-agent contract](../../docs/knowledge-agent.md) describes persisted
per-step continuations and typed slots,6-model/12-tool/120-second cumulative limits,
actual/inherited evidence dependencies and revocation. Tests also cover multi-round
retrieval, argument repair, tenant denial, inherited purge and retry/tool exhaustion.

New runs pin the bundled recipe version/digest in private execution state. Older runs keep
their admitted tool vocabulary. Recipe reads and calculations share the same cumulative
budget, durable slots, retry rules and dependency purge as knowledge reads. They do not
add a second task classifier or reset memory-planning budgets. See [task skills](../../docs/task-skills.md).

```sh
cargo test --locked -p moosuri-agent
cargo clippy --locked -p moosuri-agent --all-targets -- -D warnings
```
