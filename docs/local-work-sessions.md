# Opt-in local work-session feed (F41)

`moosuri work-sessions codex` reads one explicitly selected directory and sends
small **user-request evidence** to one explicitly selected custom HTTP source
connection. It runs once. It does not start a server, install a service/cron,
execute tools, call a model, discover a home/history directory or upload a full
transcript. This first unit supports the narrow Codex JSONL subset below;
Claude, other clients, compressed logs, databases and missing-provenance formats
are unsupported.

## Select the source and destination

Create a dedicated custom HTTP connection in **자료 연결** and save its ingest token
in an owner-only regular file. Use only this producer/state for that connection.
The destination connection UUID is required independently of the token; a token
for another connection fails before a batch is sent. The command needs no local
moosuri server installation or `--data-dir`.

```sh
moosuri work-sessions codex \
  --directory /absolute/private/chosen-codex-export \
  --origin https://your-moosuri-server.example \
  --connection-id 00000000-0000-4000-8000-000000000001 \
  --token-file /absolute/private/feed/token.txt \
  --state-file /absolute/private/feed/cursor.json
```

Replace the example UUID with your connection's actual ID. Every path is chosen
explicitly; no directory or destination has a default. The source directory and
private token/state parent directories must already exist, be canonical absolute
paths without symlinks and be owned by the current user with mode0700. Token,
state and lock files are same-owner0600 regular files with no hardlinks or
symlinks. Source regular files may be0644 within the private root but may not be
foreign-writable/hardlinked. Token and state must be separate and outside the
source directory; state must also be outside every Git checkout. Source symlinks
are skipped and opens use anchored no-follow directory/file descriptors. Every
source/private-path ancestor must be root/current-user owned, without group/world
write unless sticky; a trusted writable sticky ancestor such as `/tmp` requires
its next directory component to be current-user owned.

Only `.jsonl` regular files under the chosen root are considered. Files modified
in the last60seconds defer. Reads check device/inode, size, nanosecond modification
time, full-file SHA256 and stable descriptor/path identity before any evidence is
staged. Changed/unsafe files defer independently of stable peers. A root replacement
during scanning aborts staging. Hashes and opaque source IDs deduplicate unchanged
contents and inode replacements; absence never implies a remote delete.

One invocation visits at most20 files,64MiB total,32MiB/file,20,000 complete
records/file and16 changed turn objects. The directory bound is1024 files,
4096 entries and depth8; choose a smaller directory if that bound is exceeded.
The local scan position rotates between invocations so unchanged early files do
not starve later ones. State bounds are1024 file fingerprints,8192 object hashes
and2MiB. These are bounded scans, never a claim that all external history was read.

## Proven supported input subset

The wire shapes were checked against public `openai/codex` commit
[`b1573f102413e96a6481087922bf425231222dee`](https://github.com/openai/codex/commit/b1573f102413e96a6481087922bf425231222dee), not guessed from a private session dataset:

- [JSONL writer](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/rollout/src/recorder.rs)
  writes timestamped newline-terminated records;
  [persisted envelope](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/history/src/rollout_payload.rs)
  uses `type` and `payload`.
- [SessionMeta, EventMsg, UserMessageEvent and task lifecycle](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/protocol/src/protocol.rs)
  define root IDs, source/thread provenance, literal user message and canonical
  v1 `task_started`/`task_complete` tags.
- [TurnAttribution](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/protocol/src/turn_input.rs),
  [TUI user submission](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/tui/src/app/user_turn_submission.rs)
  and [TUI thread creation](https://github.com/openai/codex/blob/b1573f102413e96a6481087922bf425231222dee/codex-rs/tui/src/app_server_session.rs)
  establish the explicit `user` turn trigger and thread source.

The first complete line must be `session_meta` with a UUID `id`, `source:"cli"`,
explicit `thread_source:"user"`, and matching `session_id` if present. Fork,
parent, agent identity/path, inherited history base and subagent start markers
must be absent/null. This deliberately excludes exec, app automation, VSCode,
MCP, subagents and inherited/forked history. Missing provenance is unsupported;
there is no fallback that assumes a record is human.

Evidence comes only from `event_msg` → `user_message.message` inside a
`task_started` whose `turn_attribution` pins the same turn and explicitly says
`turn_trigger:"user"`, without parent/initiating-agent attribution or another root
turn. Automation/queued/unknown triggers are not evidence. `response_item`, tool,
assistant, configuration, attachments, plans and reasoning records never become
request text. Compaction clears active attribution until another proven human
start. Current public v1 tags are supported; version-wide or alias compatibility
is not claimed.

Each turn retains at most four literal prose requests of2048bytes each. Entire
messages containing credential/context/code/structured-dump markers, private
paths, URLs, control characters or long opaque identifiers are omitted rather
than summarized or partially reconstructed. This intentionally omits some benign
technical requests too. Omitted records are not proof of complete request coverage.
No token, config field, local path or tool/assistant text is selected as evidence.

A matching successful `task_complete` creates a **finished marker** bucket; that
means the local turn ended, not that the requested work succeeded. Without that
marker, a stable turn is partial and deferred until its latest user request has
been idle for one hour. A non-null completion error remains partial. An unfinished
last JSONL line is never parsed; complete prefix requests can still be partial
evidence. Malformed complete/oversized records make that file unsupported. A
later complete append updates the same opaque turn object using a semantic hash.

## Durable cursor and recovery

The producer uses the existing `/api/v1/source-ingest` status/run/batch/receipt/
finish protocol. Production accepts one exact HTTPS origin with no userinfo,
path/query/fragment, redirects, proxies, Cookie/Origin headers or automatic HTTP
retries. Each request and streamed response has time/size bounds. HTTP loopback
is enabled only through isolated unit/production-boundary fixture entrypoints,
not the shipped command or an ordinary CLI flag.

A new state requires an unused dedicated connection (sequence0/no checkpoint or
active run). The0700/0600 private state stores origin/connection/config/root
binding, hashed file/turn cursor, run intent and an immutable pending envelope.
The token is never written to state or logs. Pending selected request text is
private data required to recover its exact upload; it is removed from state after
receipt/finish confirmation. Completed state contains hashes, not request bodies.
A nonblocking process lock excludes another invocation using the same state.

File/object cursor, sequence and checkpoint advance only after the original
receipt and finish are confirmed, or that exact receipt is reconciled against the
current after-sequence/checkpoint with no active run. A restart/expiry may leave
the historical run unknown even though its batch committed; that history stays
unknown and is never rewritten as completed. A different active run or changed
sequence/checkpoint refuses reconciliation and retains local pending state. On unknown batch ACK, rerun the **same command
with the same state file**. The producer first looks up the original request UUID,
reuses the exact saved bytes only under the original active run/position, and does
not re-extract changed files under that UUID. A committed batch and lost finish
ACK recover the same receipt and retry only the original finish. Destination,
config, position, lease, auth and receipt mismatches stop with state retained.
Pause/revocation/deletion is enforced by the existing server protocol.

**Run-start ACK recovery (schema48):** the producer saves a client UUID,
expected run generation, current opaque grant binding and exact before-position
before sending optional `/runs` intent. Repeating it resolves only the same
original run ID/generation, never an unrelated active run. History pruning cannot
reuse an old composite intent because connection generation never rolls back.
There is no permanent start-key quota. UUID alone is not a global perpetual key.

If startup/expiry made that exact start inactive before any batch envelope was
staged, a later explicit invocation may pin one fresh start/batch attempt from the
same selected snapshot. It first proves original receipt absence, exact current
before-position/grant and no conflicting active run. The old unknown run remains
unknown. If a batch envelope was already staged, an inactive uncommitted attempt
stays pending; it is not silently rebound. Current authority/scope disagreement
also retains state. Existing R1 committed-batch recovery remains unchanged.

The server must support schema48 status/start fields. Old `{}` producers and
normal official pollers still work; legacy pre48 producer states whose unkeyed
start ACK was already lost cannot be retroactively identified. They stop with
state retained rather than adopting an unproven active run. Do not delete an
uncertain state file to force a new request.

Success prints only a fixed status, opaque request UUID and counters for accepted,
unchanged, examined, deferred and unsupported files. It always labels the bounded
scan a producer report/partial scan. Errors are fixed codes; they never print
source text, token, path, remote checkpoint or provider diagnostics.

[Focused synthetic evidence](reviews/2026-11-14-local-work-session-feed.md) separates
mock protocol behavior from actual Store/server integration and real-account use.
