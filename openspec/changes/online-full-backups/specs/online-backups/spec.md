## ADDED Requirements

### Requirement: Consistent live full-data capture
The system SHALL capture committed SQLite data and fully applied auth/deletion journals in one live writer job, with bounded sizes and a checked deadline.

#### Scenario: Writes after capture
- **WHEN** a memory is deleted after capture and before encryption
- **THEN** the frozen export contains the captured memory and its original journal boundary while the live writer accepts the deletion

### Requirement: Existing encrypted export contract
The system SHALL encrypt the immutable stage outside the writer using the existing format2 codec and no-overwrite publication, without including private server or recovery keys.

#### Scenario: Independent later journal restore
- **WHEN** a live-created export is restored with a trusted later deletion checkpoint
- **THEN** later deleted data remains withdrawn and old authority is quarantined

#### Scenario: Damaged stage or existing destination
- **WHEN** captured journals are malformed or the output already exists
- **THEN** export fails without plaintext publication or overwriting a retained export

### Requirement: Optional periodic lifecycle
The system SHALL enable full snapshots only through explicit operator serve configuration, run one attempt at a time, and drain the active attempt on orderly shutdown.

#### Scenario: Healthy running installation
- **WHEN** a configured worker starts
- **THEN** it creates a new encrypted snapshot, waits the configured interval after completion, and leaves normal app access available

#### Scenario: Invalid setup or later failure
- **WHEN** configuration is missing or the destination is unsafe
- **THEN** worker startup fails
- **WHEN** a later snapshot fails
- **THEN** ordinary service continues and the next attempt waits the configured interval

### Requirement: Honest checkpoint scope
The system SHALL describe local exports as captured checkpoints and SHALL NOT claim independent latestness, off-host retention or automatic receiver baseline changes.

#### Scenario: New local export
- **WHEN** a periodic export completes
- **THEN** only its export ID and key epoch are reported and an existing receiver remains pinned to its original baseline
