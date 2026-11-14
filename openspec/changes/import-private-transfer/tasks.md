## Implement and verify

- [x] Define bounded explicit ordered owner/source/memory/document transfer format.
- [x] Implement fresh staging account/data, typed Store history operations,
  retirement barriers, reference validation and complete event reconciliation.
- [x] Retain private sibling report/quarantine and bounded SQLite report artifacts;
  reject existing outputs and never import old authority/work.
- [x] Run selected actual Store/restart/backup and CLI gates; [evidence](../../../docs/reviews/2026-11-15-private-transfer-import.md).
- [x] Independent review and joined Linux Store3/actual CLI1 acceptance, including the reserved-destination repair.
- [x] Add source-grounded schema23 relation adapter and read-only SQL export recipe (actual operator export remains separate).
- [ ] Real operator export/production comparison, media/vault and
  drain/cutover/rollback remain separate; this change does not close aggregate P9.
