## 1. Scope and contract
- [x] 1.1 Remove public Kakao migration targets while retaining original source hashes/evidence and protecting the existing private collector.
- [x] 1.2 Define R29/F70, official/custom provenance and sync contracts, code targets and acceptance scenarios; validate their tracking with Rust xtask.

## 2. Shared storage and custom ingest
- [x] 2.1 Implement tenant connection/scope/config epochs, sync runs, batch receipts, provenance and opaque provider checkpoints in core/store/SQLite.
- [x] 2.2 Extract the existing source commit path without weakening manual ingest, exact replay, source revisions or evidence withdrawal; atomically commit data/receipt/local sequence/checkpoint.
- [x] 2.3 Implement one-connection ingest/status credentials, create/rotate/revoke and authenticated bounded custom HTTP push with an external synthetic sender example.
- [x] 2.4 Implement heartbeat/no-change reports, error/partial/unknown semantics, paused/stale-run fencing and disconnect keep/delete; reject incomplete snapshot retirement.

## 3. Official adapters and user visibility
- [x] 3.1 Implement opt-in official Gmail knowledge sync with existing personal OAuth, bounded scope/backfill, history invalidation and rate-limit recovery.
- [x] 3.2 Implement official Slack read/sync with personal scope, channel/thread pagination, late reply/deletion handling and rate-limit recovery.
- [x] 3.3 Implement connection management and sync history in React, source-to-connection/receipt traceability and truthful agent readiness; exclude private Kakao from the public adapter list.

## 4. Verification and release
- [x] 4.1 Pass the implemented source-connection slices of DATA01~DATA03 and DATA06~DATA08 through actual SQLite/HTTP with tenant/revoke/replay/kill/partial/secret and old-data regression checks. DATA02 Calendar/PDF/attachment and live-provider slices remain outside this change; do not mark the broader scenario group fully passed.
- [x] 4.2 Independently review credential/storage/recovery boundaries and run Linux checks; publish actual per-adapter scope/evidence before enabling support claims.

- [ ] 4.3 Optional live-provider acceptance: connect explicitly selected Google/Slack test accounts and record supported scopes/rate behavior. No live-provider result is claimed by synthetic tests.
