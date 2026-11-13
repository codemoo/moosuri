# Design

[Data connections](../../../docs/data-connections.md) defines R29/F70 and the F38
exclusion. Official adapters and custom HTTPS producers normalize into one
bounded Rust ingest service with tenant-owned connections and immutable scopes.

The current IngestBatch remains a manual API: it rejects empty batches and full
snapshots and uses an i64 local sequence. Do not reuse that integer as a remote
provider cursor. Migration9 adds opaque scoped checkpoints, independent heartbeat/no-change
receipts, run generations and typed outcomes for background sync.

Custom tokens authorize ingest/status for one connection. They do not authorize
private reads, tools or sends. The server assigns category/identity and validates
current account/config/credential epoch inside the same commit as sources,
provenance, receipt and checkpoints. External success/coverage/origin claims are
untrusted reports. The receipt attests local commit only.

Full-snapshot retirement stays unavailable until scope/completeness/finalization
is separately implemented and tested. Imported content never grants agent authority.
Current source deletion, evidence withdrawal and journal semantics must survive
extracting the common batch-commit function. Destructive operations and late data
remain fenced. Network work never holds the SQLite writer transaction.

The audit registry is planning metadata, not a runtime capability registry. Actual
availability must come from shipped adapter/version, personal connection/scope
and tested capability; a planned Slack row cannot enable tools or say connected.

The five-second worker performs bounded due-connection scans without model calls.
Gmail reuses a personal OAuth binding through a background credential carrier and
the shared refresh lock, never a fabricated login session. Slack verifies a user
token and polls a bounded channel/thread window; `polled` does not certify all remote
deletions. Custom completion is explicitly a producer report. Normal page-budget
continuation is partial without error backoff. History/checkpoint reset is explicit
and preserves deleted-source suppression.

The React settings route exposes current usability, pause/disconnect, rebind/reset,
one-time token delivery, safe history and receipt-linked provenance. Account changes
discard transient credentials and stale requests. Browser routing is verified through
the real embedded server, in addition to component behavior checks. Provider account
setup and live acceptance remain separate from synthetic acceptance.
