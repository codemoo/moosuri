# Supervised text runtime

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
Unknown phase is not inferred to be final. Tools, memory and attachments remain
unavailable. Exact reported model identity and a successful transport terminal are
required for completion. There is no automatic retry, model change or fallback.

`model_client` provides the same validated hosted/local constructor to the runtime
and admin provider probes. It performs no network IO and never logs credentials.

Synthetic file-backed DB and actual loopback HTTP tests cover partial visibility,
disconnected subscribers, duplicate ACK dispatch, streamed errors, refusal/limit/
identity states, cancel/delete/logout fences, shutdown/drop, reopen without replay,
UTF-8 patch bounds, timeout/truncation/connection failure, transient store pressure,
exited-worker settlement and wire/late phase behavior. These are implementation tests,
not live-model quality or full agent evaluations.

```sh
cargo test --locked -p moosuri-agent
cargo clippy --locked -p moosuri-agent --all-targets -- -D warnings
```
