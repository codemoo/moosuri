## Boundary

An operator restores/replays the original age/custom-pg_dump using old tooling in
isolation. A product-authored SQL recipe emits relation inventory, rows and complete
marker in one read-only snapshot. This implementation never executes SQL or reads
old runtime configuration. A single explicit owner binds the global legacy DB.

## Projection and suppression

Schema/checksums are exact; unsupported relation bodies remain only in original
input. Source binding/version, claim/item/current identity, active support and
artifact visibility/reference checks select current projections. Legacy N maps to
new1, with N recorded; no missing history is fabricated. Suppressed identities or
withdrawn memories withhold implicated raw source scopes; unscoped suppression
withholds all source admission. Current valid owner corrections may survive.

## Publication

The adapter emits bounded events into a private temporary file and uses the accepted
fresh-account importer. Its optional report hook publishes adapter reconciliation
inside the same staged report directory before data publication. Any generated
event unexpectedly rejected by Store aborts all publication. No schema or dependency
change is required. See docs/legacy-relations.md for bounds and remaining gates.
