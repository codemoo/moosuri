## 1. Specify
- [x] 1.1 Define authoritative file backup and exact scheduler policies with OPS03/AUTO01/AUTO02 oracles.

## 2. Implement and verify
- [ ] 2.1 Complete external authoritative-file publication/manifest/pins and GC protection if bytes move outside SQLite. Current image/UTF-8/artifact bytes are canonical SQLite state; this task is not evidence that current exports omit those bytes.
- [x] 2.2 Implement public-recipient stopped setup/export and retained key epochs; bounded independent identity inputs verified in [export evidence](../../../docs/reviews/2026-11-13-stopped-encrypted-export.md).
- [ ] 2.2a Complete independently latest off-host journal/freshness and ongoing recovery without original host-local secrets or restored-vault dependency.
- [x] 2.3 Implement exact sealed-current encrypted checkpoint restore to a new root, including SQLite images/files/artifacts and quarantine; [restore](../../../docs/restore.md) states the bounded contract.
- [ ] 2.3a Complete historical export replay/latest-deletion recovery and PDF fixtures once PDF exists; reject missing/corrupt bytes and wrong/missing keys before/after rotation. Sealed cutover does not complete this contract.
- [x] 2.4 Verify fixed local one-shot/interval/daily/weekly wall-clock/DST/misfire rules against exact UTC/count oracles; [reminder evidence](../../../docs/reviews/2026-11-13-local-reminders.md).
- [x] 2.5 Serialize fixed local reminder pause/edit/revoke with atomic publication; held-claim and late-result coverage in the same reminder evidence.
- [ ] 2.5a Complete broader external dispatch commitment/revocation contracts. Schema28 read-only recurring mission integration is accepted within the bounded [recurring evidence](../../../docs/reviews/2026-11-13-recurring-missions.md); it does not complete broader external effects.
- [x] 2.6 Record bounded real SQLite process/restart and sealed-current restore checks in the linked evidence.
- [ ] 2.6a Run remaining ongoing/historical recovery and applicable external-effect kill/restore gates before full support claims.

These splits reconcile already recorded evidence; this documentation change does not rerun product tests or complete the aggregate recovery contract.
