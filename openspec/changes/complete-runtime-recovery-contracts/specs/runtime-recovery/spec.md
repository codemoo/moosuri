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
