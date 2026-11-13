## ADDED Requirements

### Requirement: Permanent stopped source retirement
Checkpoint SHALL require explicit seal intent, a stopped initialized configured
supported installation and complete==applied journal heads. One FULL-sync DB
commit SHALL permanently adopt retirement under its exclusive writer lock.
Runtime, account recovery, setup and ordinary export SHALL reject sealed state
before source mutation, replay, key handling, worker startup or enrollment.

#### Scenario: Checkpoint output fails after adoption
- **WHEN** capture, encryption, syncing or output publication fails after seal commit
- **THEN** the source remains retired and retry uses the original seal/checkpoint IDs
- **AND** no generic initialization or account recovery can activate it

### Requirement: Exact current-state snapshot coverage
Checkpoint SHALL capture the full sealed SQLite DB and all three exact journals
in a distinct fixed six-entry package, with typed coverage and byte hashes/sizes.
Original four-/five-entry export codecs SHALL remain unchanged.

#### Scenario: Source update withdraws derived content without a new deletion head
- **WHEN** current source/memory state has changed since an older export
- **THEN** the sealed checkpoint includes the actual current withdrawal/index state
- **AND** the older DB with matching journal heads is not a substitute

### Requirement: Immutable ready ciphertext and durable publication
Finalized encrypted staging SHALL be file/directory-synced before its immutable
receipt commits. Publication SHALL never replace conflicting outputs, SHALL sync
ciphertext before head and SHALL report complete only after both are durable.
Readable bytes after a prior sync error SHALL require successful retry syncing.

#### Scenario: Publication succeeds but acknowledgement is lost
- **WHEN** retry finds the same finalized outputs
- **THEN** it validates/re-syncs the exact stored bytes and returns the same IDs
- **AND** a different existing file is preserved and rejected

### Requirement: Explicit manual cutover trust boundary
Checkpoint/head purposes SHALL distinguish sealed cutover from ordinary export.
A local head SHALL NOT be described as authenticated, independently latest or
successfully transferred off-host. No restore/import or ongoing disaster recovery
support SHALL be claimed by this unit.

#### Scenario: Operator retains only an adjacent replaceable head
- **WHEN** a head has not been carried through a separately trusted recovery channel
- **THEN** matching hashes alone do not establish trusted freshness or restore readiness
