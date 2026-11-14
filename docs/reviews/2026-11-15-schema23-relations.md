# Schema23 relation adapter evidence

Scope: product-authored isolated-snapshot export recipe and local JSONL adapter,
fresh importer integration, CLI and documentation. No actual legacy DB, backup,
credential, environment or service was accessed. Only allowed local source/schema
files informed contract checksums/names and conversion decisions. No private code
was transferred to Linux or copied into this repository.

Native Rust 1.89 selected gates:

- Store adapter3 passed: actual current source/memory/artifact import, exact quote
  references, current legacy7→new1/no old7, all-row accounting, unknown-payload
  exclusion, source disconnected/no fabricated provenance, actual age export;
  scoped/unscoped suppression and no invented prior value; schema/trailer/checksum,
  stale support and original-row whole-input limits.
- Actual CLI1 passed: new fresh account, complete unknown-row reconciliation,
  unchanged input, no unsupported-payload leakage and retry/no-overwrite behavior.
- Final count gate1 passed: relation count mismatch, stale support, exact schema,
  missing trailer, 4097 original rows and whole-input rejection at 129 converted
  objects. Final suppression gate1 passed after adding malformed empty support
  scope: all source admission is withheld in that case.

The SQL recipe was reviewed statically only. Actual psql execution, original
restore/replay consistency and production comparison are explicit operator gates.
This unit does not close aggregate migration/media/vault/cutover requirements.

## Joined Linux and independent acceptance

The integrated source passed Linux adapter Store3 and actual CLI1. Root source
review accepted exact schema/projection/reference/suppression/report boundaries.
A SQL-only completeness correction replaces wildcard `pg_%` matching with an
exact `pg_` prefix in all three catalog queries, keeping ordinary `pgdata` schemas
in the inventory. No real SQL execution is claimed. Strict Clippy found one
nested-if style issue; the equivalent let-chain correction then passed Clippy
and the embedded-web binary build. Unchanged Store/CLI checks were not repeated.
Old services, data and credentials remained untouched; CI stays disabled.
