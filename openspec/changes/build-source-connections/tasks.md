## 1. Scope and contract
- [x] 1.1 Remove public Kakao migration targets while retaining original source hashes/evidence and protecting the existing private collector.
- [x] 1.2 Define R29/F70, official/custom provenance and sync contracts, code targets and acceptance scenarios; validate their tracking with Rust xtask.

## 2. Shared storage and custom ingest
- [ ] 2.1 Implement tenant connection/scope/config epochs, sync runs, batch receipts, provenance and opaque provider checkpoints in core/store/SQLite.
- [ ] 2.2 Extract the existing source commit path without weakening manual ingest, exact replay, source revisions or evidence withdrawal; atomically commit data/receipt/local sequence/checkpoint.
- [ ] 2.3 Implement one-connection ingest/status credentials, create/rotate/revoke and authenticated bounded custom HTTP push with an external synthetic sender example.
- [ ] 2.4 Implement heartbeat/no-change reports, error/partial/unknown semantics, paused/stale-run fencing and disconnect keep/delete; reject incomplete snapshot retirement.

## 3. Official adapters and user visibility
- [ ] 3.1 Implement opt-in official Gmail knowledge sync with existing personal OAuth, bounded scope/backfill, history invalidation and rate-limit recovery.
- [ ] 3.2 Implement official Slack read/sync with personal scope, channel/thread pagination, late reply/deletion handling and rate-limit recovery.
- [ ] 3.3 Implement connection management and sync history in React, source-to-connection/receipt traceability and truthful agent readiness; exclude private Kakao from the public adapter list.

## 4. Verification and release
- [ ] 4.1 Pass DATA01~DATA03 and DATA06~DATA08 through actual SQLite/HTTP with tenant/revoke/replay/kill/partial/secret and old-data regression checks.
- [ ] 4.2 Independently review credential/storage/recovery boundaries and run Linux plus opt-in provider checks; publish actual per-adapter scope/evidence before enabling support claims.
