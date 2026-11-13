## ADDED Requirements

### Requirement: Native recovery identity preparation
The CLI SHALL generate a standard X25519 age recovery identity into a new owner-private file outside known source and installation directories, without opening an installation. It SHALL print only the public recipient and require file and directory sync before reporting success.

#### Scenario: Prepare recovery before installation
- **WHEN** the operator runs `backup keygen` in an existing private recovery directory
- **THEN** one new0600 identity file is published without overwrite and only its public recipient is printed
- **AND** a missing installation remains missing, linked/nonprivate/source paths fail, and the identity is usable by standard age implementations

### Requirement: Stopped existing-installation export admission
Export and setup SHALL require an initialized supported existing installation and
exclusive existing writer lock. They SHALL NOT initialize, migrate, replay runtime
work or mutate original DB/journal files while rejecting invalid inputs.

#### Scenario: Committed data remains only in WAL
- **WHEN** a stopped installation has committed pages in WAL after process death
- **THEN** SQLite Backup incorporates those pages and source WAL/SHM are not deleted

### Requirement: Independent identity and retained public recipient epochs
Setup SHALL verify a separately supplied operator-held X25519 identity using a
synthetic age challenge before atomically activating its public recipient epoch.
Only public recipients, retained epochs and a persistent installation UUID SHALL
be stored; missing/corrupt installed metadata SHALL NOT reset configured history.

#### Scenario: Key rotation
- **WHEN** the operator successfully verifies a new identity
- **THEN** a new epoch becomes active and older recipient epochs remain retained
- **AND** old ciphertext still requires its corresponding independently retained identity

### Requirement: Complete private encrypted export publication
An export SHALL contain a verified SQLite snapshot, bounded encrypted manifest and
both exact snapshot-era journals, with hashes/sizes and validated snapshot prefix
watermarks. It SHALL use standard age-v1 recipient encryption and fixed regular tar
entries. Publication SHALL not overwrite existing files and SHALL report complete
only after encryption finalization and durable file/directory publication.

#### Scenario: Missing bytes, wrong identity or interrupted publication
- **WHEN** source integrity, supplied setup identity, byte limits or output finalization fails
- **THEN** no plaintext fallback or successful complete-export report is issued

### Requirement: Export does not imply historical restore
The CLI and documentation SHALL distinguish snapshot journals from an independent
latest journal and SHALL leave authoritative media inventory explicitly empty.
Provider keys, operator identities and user vault secrets SHALL NOT enter exports.

#### Scenario: Original provider key is unavailable
- **WHEN** only the stopped DB/journals/installed metadata and public recipient remain
- **THEN** encrypted export remains possible without decrypting server credential ciphertext
- **AND** no restore or credential reactivation is claimed
