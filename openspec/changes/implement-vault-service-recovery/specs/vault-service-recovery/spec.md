## ADDED Requirements

### Requirement: Retire the exact stopped ciphertext source before checkpoint copying
The service SHALL hold the stopped writer lock and verify current SQLite/journal identity and chain before sealing. It SHALL fsync the seal and its directory before any source copy, including semantic replay, and verify semantic operation history before publication. Runtime SHALL reject sealed roots; failures SHALL neither unseal sources nor truncate, adopt or repair unknown journal state.

#### Scenario: Checkpoint output fails after source retirement
- **WHEN** output publication or directory sync fails after source seal durability
- **THEN** the source stays retired and a subsequent exact checkpoint retry may use fresh output paths without restoring source serving authority

### Requirement: Verify the complete bounded encrypted current checkpoint
The service SHALL age-encrypt a fixed bounded SQLite/journal archive, excluding arbitrary files and unlock secrets. Restore SHALL verify the exact trusted ciphertext/head, age authentication through EOF, schema/inventory, journal identity/sequence/head, SQLite integrity and semantic CAS, receipts, tombstones and nonce reservations before publication. The head SHALL be described as operator trust input, not independently current/off-host proof.

#### Scenario: A snapshot lacks a reservation or has a journal gap
- **WHEN** ciphertext/DB metadata hashes match but an operation reservation/receipt/tombstone is absent or the journal is missing, ahead or truncated
- **THEN** checkpoint or restore fails closed and preserves original input evidence

### Requirement: Quarantine old authority and publish a fresh service identity
Restore SHALL clear sessions and durably cancel each pending operation while retaining receipts, ciphertext, tombstones and reservations. It SHALL bind a fresh DB/journal identity to exact-checkpoint provenance, fsync private staging and publish without overwrite. Exact retries SHALL preserve subsequent destination work. The contract SHALL disclose that restored copies can accept fresh grants without a global singleton guarantee.

#### Scenario: Old pending operation is retried after restore
- **WHEN** an authenticated fresh client retries the exact imported pending operation or uses its cancelled reservation for different bytes
- **THEN** its immutable receipt remains aborted or reservation conflict prevents mutation and no old session authorizes execution

### Requirement: Reject other-UID-mutable paths and reserved destinations
All database and recovery path admission SHALL reject symlinks, foreign-owned existing entries and group/world-writable nonsticky directory ancestors. Root/current-UID owned sticky ancestors MAY be traversed only with root/current-UID owned next existing entries; final private owner and mode requirements remain. Restore SHALL share the runtime reserved-stage-name predicate and reject reserved destination basenames before checkpoint decryption or extraction.

#### Scenario: Writable ancestor above a private source or destination
- **WHEN** a private0700 source, input parent or destination parent lies below an existing0770/0777 nonsticky directory ancestor or a foreign-owned ancestor
- **THEN** ordinary database admission and recovery reject before source sealing, file copying or destination publication and preserve original evidence

#### Scenario: User selects an internal staging basename
- **WHEN** a restore destination basename begins `.vault-recovery-stage-`
- **THEN** restore rejects before checkpoint decryption, creates no destination or stage and leaves ciphertext/head/identity unchanged
