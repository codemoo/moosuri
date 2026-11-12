## 1. Specify
- [x] 1.1 Define authoritative file backup and exact scheduler policies with OPS03/AUTO01/AUTO02 oracles.

## 2. Implement and verify
- [ ] 2.1 Implement immutable file publication and snapshot manifest/pins; show GC cannot delete referenced backup bytes.
- [ ] 2.2 Implement public-recipient backup setup/export and retained key epochs; verify independent recovery inputs with no host-local secret or restored-vault dependency.
- [ ] 2.3 Produce complete encrypted archives and restore image/PDF/artifact fixtures into an empty data directory; reject missing/corrupt bytes and wrong/missing keys before/after key rotation, preserving deletion tombstones.
- [ ] 2.4 Implement wall-clock/interval/DST/misfire policies; assert exact UTC occurrences and execution counts with a fake clock.
- [ ] 2.5 Serialize pause/edit/revoke with dispatch commitments; verify held claims and late results cannot create duplicate effects.
- [ ] 2.6 Run relevant real SQLite process-kill/restart/restore checks and record measured results before claiming support.
