## Implementation

- [x] Inspect allowed local schema23 source contract without real data/runtime access.
- [x] Add read-only relation export recipe and exact relation/migration contract.
- [x] Implement bounded current-projection joins, suppression admission policy,
  per-row reconciliation, report documents and fresh importer publication hook.
- [x] Add actual CLI and synthetic Store/CLI/count/suppression checks.
- [x] Document current-only semantics, operator route and production gates.
- [x] Independent Linux integration and review (main-owner gate).
- [ ] Operator isolated PostgreSQL restore/export and production comparison;
  media/vault migration and complete drain/cutover/rollback.
