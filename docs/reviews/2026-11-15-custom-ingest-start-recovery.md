# Recoverable custom-ingest start — 2026-11-15

F23/F41 now support optional durable start intent on the existing bearer-only
`POST /api/v1/source-ingest/runs`. Existing zero-byte and `{}` clients retain their old response,
and official pollers retain their original Store lease API. No new server, model
call, tool execution, service/cron, credential scope or dependency was introduced.

The start identity is composite: current owner/connection/config/grant binding,
expected run generation and client UUID, with exact before-sequence/checkpoint.
Status adds run_generation and an opaque non-authorizing start_binding. It is a
one-way tag for the current grant, not a token or capability; bearer authorization
remains mandatory. Token/nonce values never enter intent state or safe receipts.
The Store rechecks current authority inside its immediate transaction.

Schema48 maps a start UUID/canonical digest to its original run. Exact retained
replay returns the same ID/generation/current historical state without allocating
or renewing a lease. Startup/expired unknown history stays unknown. Metadata
cascades with normal run pruning; monotonic connection generation prevents an old
pruned intent from allocating again. There is no permanent UUID quota that stops
ordinary periodic synchronization. UUID alone is explicitly not a forever-global
idempotency key. New expected generation means a new explicit composite intent.

The local producer fsyncs its complete typed intent before first dispatch. Lost
start ACK repeats that same intent. On a later explicit invocation, a known
inactive start can stage one fresh start/batch attempt only when no batch envelope
was ever staged, the original batch receipt is absent, the current before-position
and grant still match and no conflicting run is active. Selected snapshot entries
remain fixed. Staged inactive batches retain their immutable envelope instead of
rebinding it. R1 committed-batch/current-after-position reconciliation remains;
unknown history is not relabeled successful. Old pre48 unkeyed uncertain starts
cannot be retroactively identified.

## Focused actual checks

All input and accounts were synthetic and isolated. No real session dataset,
external provider/model/mail/Push, production DB/service or cron was used.

- **Store5 passed:** three new composed start cases (exact replay, restart/expiry,
  pruning/generation, explicit schema47 upgrade and scope/grant/reset fences),
  existing source token/tenant isolation, and actual sealed-current restore.
  A foreign grant tag fails; the same client UUID in another owner/connection is
  accepted only as that other connection's correctly bound composite identity.
- **Production router3 passed:** existing producer/browser isolation and commit/
  revocation cases plus new keyed start HTTP replay, mismatch, token rotation and
  legacy empty-object compatibility.
- **Actual local-file→Store/router producer2 passed:** R1 committed-receipt restart
  case and one three-mode start-fault fixture. The modes fault start ACK after
  real commit while run stays live; lose that ACK then reopen Store unknown; and
  persist the ACK but fail status before envelope staging, then reopen unknown.
  Repeated original intent never allocates a second original generation. Only a
  new explicitly staged composite intent creates the inactive retry generation.
  Each mode commits exactly one source batch/source receipt; old unknown history
  remains unchanged. The HTTP shim only faults responses; actual start/ingest/
  receipt/finish authority runs through the production router and Store.
- **Connector5 passed:** existing selected-format/privacy, immutable uncertain
  batch/finish, partial append/inode and private-path cases under the new packet.
  These do not broaden supported client formats or establish real-account use.

```sh
cargo test --locked -p moosuri-store --lib -- source_connections::start_recovery actual_current_schema_restores tenant_identity_exact_replay_rotation --test-threads=1
cargo test --locked -p moosuri-server --test http_boundary source_connections:: -- --test-threads=1
cargo test --locked -p moosuri --features test-fixtures --bin moosuri work_sessions::recovery_tests -- --test-threads=1
cargo test --locked -p moosuri-connectors --lib work_sessions::tests -- --test-threads=1
```

Changed-library Clippy with `-D warnings` and the full CLI canonical TypeScript
export passed. Generated main+worker TypeScript checks, formatting, strict OpenSpec
and whitespace checks passed. Normal
and sealed-restore schema48 hooks are included; predecessor fixtures drop the new
sidecar before reconstructing their explicit older DDL. Actual restore preserves
start metadata while quarantining token/history and advancing generation; old
bearers fail after recovery. Schema-derived account deletion scopes the new table.

## Remaining limits

Independent review and root joined Linux integration passed after one compatibility
repair. The original `Option<Json<_>>` extractor rejected legacy zero-byte bodies
with a JSON Content-Type before the handler. Root switched to the existing bounded
16KiB Bytes extractor and strict parsing for nonempty input. The real router case
covers `{}`, empty bytes with/without the header, malformed JSON and unknown fields;
invalid bodies leave generation1 available. The independent reviewer accepted the
exact repaired source and found no remaining P1/P2 issue.

Joined Linux checks passed: Store start3, schema/restore20, router3, selected-local
connector5 and actual Store/router producer2. After the handler repair, only the
affected router3, all-target Clippy (`-D warnings`) and default build were rerun and
passed. Exact generated TypeScript matches the integrated source, retaining timing/
recall/skill contracts, and web TypeScript checks passed. Publication additionally
runs formatting, plan/OpenSpec validation and source/new-history secret scans.
These counts overlap the author's cases and are not extra independent quality
samples. CI remains disabled.
Real HTTPS/account/client exports and broader history/client formats remain
unverified. A pruned historical key or legacy unkeyed unknown start stops rather
than inventing a new identity; an inactive staged unconfirmed batch is not silently
replaced. This is the existing custom producer protocol, not generalized job/send
idempotency or autonomous work.
