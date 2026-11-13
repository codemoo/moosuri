# Integrated sealed checkpoint and restore evidence

2026-11-13. Final assistant schema20 includes artifacts16, local reminders17,
account management18, sealed checkpoints19 and exact-current restore20.
This supersedes the provisional schema numbering in the earlier
[isolated checkpoint record](2026-11-13-sealed-checkpoint.md).

The checkpoint captures the complete stopped current SQLite and three journals.
Restore requires the exact ciphertext, a separately trusted matching head and
independent age identity. It verifies authentication through EOF, strict bounded
archive membership, file hashes, DB/schema/installation and complete journal
closure before forward migration, quarantine and native no-replace publication.
All seven actual artifact tables, deletion/withdrawal and committed receipts are
preserved. Schedule claims are quarantined; suspended accounts stay suspended.
Fresh account enrollment never restores old credential or dispatch authority.

Independent storage/auth review first found a late-publication durability gap:
after successful rename and failed parent fsync, fresh runtime or offline writer
admission could proceed without re-syncing the root name. The shared admission
barrier now syncs the parent under the exclusive writer lock before mutation or
new authority. Injected failure covers Store, operator recovery, backup setup,
export, checkpoint and matching restore; destination bytes/modes stay unchanged.
The review independently reran that regression and same-input retry after fresh
password enrollment/new memory creation. Both passed, and the repair was accepted.
A separate earlier stale-absent-root admission race was also closed and reviewed.

Final macOS cutover suite:18 passed,1 optional reference case ignored; the selected
standard Go age binary then independently decrypted the six-entry checkpoint in
that explicit case. Final Linux x86_64 checks:cutover18, backup9, recovery7 and
CLI18 passed;3 optional reference-tool cases were ignored. Before the narrow
repair, Linux destructive-journal7, schedules10 and account-management5 tests also
passed on the integrated schema20. These are scoped observations, not a sum of
unique global product tests. Store/CLI warnings-denied clippy, fmt, plan links and
strict OpenSpec were checked. Frozen source hashes were verified on Linux.

Other outcome tests cover malformed/truncated/tampered archive inputs, occupied
roots, source byte preservation, abrupt child-process exit, all restore boundary
retries, immutable lineage/provenance and no repeated quarantine on matching retry.
The worker's broader isolated Store220/CLI13 gates are inherited implementation
evidence; they are not relabeled as independently run checks. This is error and
process-interruption testing, not physical power-loss simulation.

No production root, real account, credential, provider, mail, collector or cron
was touched. This unit is an intentional sealed-current cutover, not arbitrary
historical export replay, an off-host backup target or ongoing disaster recovery.
The source cannot be unsealed. New media/vault inventories require their own
integration proof. CI remains disabled.
