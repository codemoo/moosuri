# Sealed current checkpoint — isolated evidence

2026-11-13. Reserved0017, based on integrated schema15. Standalone current/schema
threshold is16; final root must integrate artifact16 then set this migration,
cutover minimum and threshold to17 together (current schema17 at that integration).
The minimum stays17 if schedules18/accounts19 are subsequently added; capture uses
the actual compiled current schema and codecs reject other capture versions until
explicitly supported. Sealed roots are blocked before any forward migration. Coverage baseline15
is unchanged. No dependency, crypto primitive, restore/import or off-host action.

A monotonic STRICT singleton records active/sealed/checkpoint_ready. The permanent
seal fixes opaque IDs/recipient and exact complete==applied journal heads. Runtime
and recovery hold the same lock from read-only DB/WAL admission into Store startup;
source chmod/create/replay/key handling happens only after active admission. Ordinary
backup setup admits before reading/challenging identity input. Backup keygen remains
installation-independent. No public allow-sealed flag is introduced.

The exact sealed full SQLite snapshot captures invalidated current memory and FTS
withdrawal after actual source ingest/update, despite an unchanged successor head;
another tenant's memory remains active. A saved earlier DB demonstrates the older
active memory. This is capture evidence, not historical replay or import. The exact
artifact16 withdrawal/DeleteArtifact proof is a main integration gate, not simulated
by duplicate tables/API. Schedules18 and lifecycle19 require explicit future restore
quarantine/preserved suspension; unknown schemas are not inferred safe.

Focused synthetic tests cover six-entry tar/age payload and head hashes, archived
sealed DB, missing provider key, direct Store/web-profile/recovery/setup/create
fences, live clone lock/invalid paths/pending intents before adoption, nineteen
file/directory-sync/phase/publication fault boundaries and stable IDs, marker loss/
prefix/conflict, stage corruption/prefix repair/exact-copy recovery, active DB-only
rollback, missing installed singleton and immutable state transitions. Separate
child processes exit abruptly after the seal and ready commits without Rust Drop,
then committed WAL capture/retry preserves the seal and ready hash. These are
process/error tests, not simulated physical power loss.

Native no-replace rename is used on macOS/Linux. No hard-link publication window
exists in the new checkpoint producer; old export/keygen publication is untouched.
Matching existing outputs are bounded, checked and re-synced, and different bytes
never overwrite. Only currently tracked matching temp inodes are cleaned; unknown
stages and abrupt-process partial residue are not recursively deleted.

The first broad regression exposed setup-path error precedence and a recovery
preflight attempting initial lock creation on an unrelated DB, plus unrelated-root
error classification. These were fixed in production admission/path ordering;
assertions were retained. The current-schema destructive-journal downgrade fixture
now drops the cutover table before synthesizing a genuine pre-feature schema.
Linux's older format1 empty-file fixture uses create_file/fsync rather than inheriting
0644 from fs::write; its literal codec bytes/assertions remain unchanged.

Final Store regression:186 passed,0 failed,2 explicitly ignored reference-age
cases. Focused cutover suite:8 passed,1 reference ignore. Native CLI suites:6 passed,
1 reference ignore across checkpoint/backup/recovery/installation (no double count
of shared feature tests). The installation case includes actual synthetic loopback
HTTP startup/login/restart after the unchanged web assets were built locally.
Workspace/all-target warnings-denied clippy, fmt check, plan360 links and strict
OpenSpec pass. Exact commands, source and log hashes are recorded in the sibling
checkpoint delivery evidence. Local reference Go-age execution is explicitly
optional/ignored without a selected binary. Main owns independent review, merged
Linux acceptance, artifact16 proof and deciding when to expose irreversible sealing.
No real installation, provider, network, credential, account management or remote
action was used. No commits/push/deployment or child delegation occurred.
