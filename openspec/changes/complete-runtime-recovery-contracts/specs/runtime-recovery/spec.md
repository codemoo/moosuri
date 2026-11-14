## Purpose

Preserve every authoritative user file across backup and restore, and recover scheduled work with deterministic timing and ownership rules without resurrecting deleted data or duplicating external effects.

## ADDED Requirements

### Requirement: Complete backup snapshot
A complete backup MUST contain its SQLite snapshot, authenticated file manifest and every referenced authoritative blob, protected against garbage collection until archive completion or failure is resolved.

#### Scenario: Deletion during backup
- **WHEN** a user deletes an attachment after the backup snapshot is pinned
- **THEN** copying remains internally consistent and restoring with the latest deletion journal does not expose that attachment

### Requirement: Empty-directory restore verification
Restore MUST verify authoritative blob hashes, sizes and ownership in an empty data directory before becoming ready; missing or corrupt required bytes MUST leave recovery incomplete.

#### Scenario: Database is valid but a PDF is missing
- **WHEN** the database passes integrity and foreign-key checks but a manifest PDF is absent
- **THEN** restoration remains recovery_required rather than reporting success

### Requirement: Deterministic DST and interval timing
Recurring wall-clock schedules MUST skip nonexistent local times and execute a repeated local time once at its first offset. Interval schedules MUST use a persisted UTC anchor independent of completion time.

#### Scenario: Repeated New York local time
- **WHEN** a daily 01:30 America/New_York schedule reaches the 2027-11-07 fold
- **THEN** exactly one occurrence is due at 05:30Z and none at 06:30Z

### Requirement: Explicit missed-occurrence policy
Recurring schedules MUST coalesce downtime misses to the latest eligible occurrence. Valid one-shot internal work MUST run once delayed; fixed external sends MUST expire after their five-minute dispatch window.

#### Scenario: Several recurring checks missed during downtime
- **WHEN** the service restarts after three eligible scheduled checks were missed
- **THEN** at most the latest check is queued and older misses are recorded as skipped

### Requirement: Pause and edit fencing
The writer MUST serialize schedule revisions and pause state with dispatch commitment. Uncommitted stale claims MUST NOT dispatch, pause-period occurrences MUST NOT be replayed on resume, and committed effects MUST retain their original identity and receipts.

#### Scenario: Edit after claim before commitment
- **WHEN** an occurrence is claimed and the schedule revision changes before its dispatch commitment
- **THEN** the stale claim is fenced and only future occurrences use the new revision

### Requirement: Independent backup key recovery
Encrypted backups MUST have operator-held recovery identities and key-epoch metadata available outside the original host and the vault being restored. Required journal verification material MUST also survive host loss. Outer backup keys MUST NOT decrypt inner user vault items.

#### Scenario: Original host and configuration are lost
- **WHEN** only documented offline identities, archives and latest independent journal/verification material remain
- **THEN** an empty host can restore the encrypted data while inner vault contents still require the user's own master or recovery key

#### Scenario: Recovery across outer key rotation
- **WHEN** retained archives or required journal segments use both old and new outer key epochs
- **THEN** the recovery kit retains the identities needed for both until verified re-encryption or valid retention expiry

#### Scenario: Backup identity unavailable
- **WHEN** the matching recovery identity is missing or decryption fails
- **THEN** recovery reports backup_key_missing or backup_decryption_failed without plaintext fallback or substituting a user's vault master key

### Requirement: Explicit manual historical recovery trust
Manual historical recovery MUST bind both retained export and later journal ciphertexts to an independently retained head digest. It MUST verify full auth/predecessor/successor lineage and byte-prefix extension, apply supported withdrawals before publication, and quarantine restored authority. Local files and timestamps MUST NOT be presented as proof of independent latestness or off-host retention.

#### Scenario: Stale journal against the current independently retained pin
- **WHEN** an older journal/head is supplied with the newer trusted head digest
- **THEN** restore fails before publication; a caller supplying both a stale head and stale digest is explicitly outside local rollback detection

#### Scenario: Source continues after manual journal publication
- **WHEN** a stopped active source publishes a complete manual journal checkpoint
- **THEN** it remains active-capable afterward, while publication failure never advertises a missing head as recovery completion

#### Scenario: Historical revisions differ from deletion-time revisions
- **WHEN** a supported withdrawal targets an older snapshot revision or a UUID created after that snapshot
- **THEN** restore conservatively withdraws matching snapshot content or skips an absent UUID without reconstructing later additions or granting old execution authority

### Requirement: Explicit periodic live publication boundary
Optional live journal publication MUST capture one complete applied view on the existing writer, release that writer before remote transfer, and obtain a separately operated receiver acceptance receipt before reporting the captured boundary protected. Local writes MUST NOT gain per-action approval steps. Periodic publication MUST expose its recovery-point gap and MUST NOT claim zero-loss latest-deletion recovery.

#### Scenario: Concurrent deletion and live capture
- **WHEN** a deletion and capture are queued on the writer
- **THEN** the capture contains either the complete preceding state or complete following journal state, never mixed applied heads

#### Scenario: Receiver commits before transport ACK is lost
- **WHEN** journal artifacts and independent receipt are durable but the sender receives no ACK
- **THEN** retry validates exact prefix/content and returns the same accepted receipt without advancing unchanged journals

#### Scenario: Remote target unavailable or rolled-back source publishes
- **WHEN** the target is unavailable or the source submits an older/forked prefix
- **THEN** ordinary source work continues, protection reports unavailable/lagging, and independent latest acceptance never rolls backward
