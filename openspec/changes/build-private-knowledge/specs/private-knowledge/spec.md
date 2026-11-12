## ADDED Requirements

### Requirement: Source ingestion is scoped and idempotent
The system SHALL bind each batch to an authenticated tenant, immutable collection scope,
request UUID, payload digest and expected cursor. Data and next cursor SHALL commit together.

#### Scenario: Lost acknowledgement
- **WHEN** the same accepted batch is sent again
- **THEN** the original receipt is returned without duplicating sources or advancing the cursor again

#### Scenario: Changed or stale batch
- **WHEN** a request UUID has a different payload or a fresh batch uses a stale cursor
- **THEN** the system rejects it without changing source text or cursor

### Requirement: Memories retain validated provenance and temporal corrections
The system SHALL support explicit assertions or exact source revision/quote evidence,
stable memory IDs, revision CAS, explicit historical reads and temporal current search.

#### Scenario: Future correction then expiry
- **WHEN** a correction begins in the future and expires later
- **THEN** the prior fact remains current before its start, the correction applies within its validity, and the prior fact does not revive after expiry

#### Scenario: Foreign or changed evidence
- **WHEN** evidence crosses tenants, has a mismatched span, or its source changes
- **THEN** foreign/mismatched evidence is rejected and stale dependent memory revisions are not current facts

### Requirement: Destructive scopes are durable and distinct
The system SHALL distinguish source deletion, collection keep/delete disconnection,
memory forgetting and conversation deletion, with opaque journal replay before readiness.

#### Scenario: Forget with retained source
- **WHEN** a user forgets a memory
- **THEN** its plaintext revisions and search entries are removed while the original source remains and stale replay cannot restore the forgotten item

#### Scenario: Journal ahead of SQLite
- **WHEN** a destructive journal entry was synced but the database transaction did not commit
- **THEN** startup applies that entry before serving private knowledge

### Requirement: Search and editing preserve the account boundary
The system SHALL filter private search candidates by tenant and supplied scope/time/speaker,
bound pagination to its visibility generation, and fence stale browser operations.

#### Scenario: Another user or stale page
- **WHEN** an administrator requests another user's evidence or a page cursor predates a mutation
- **THEN** private evidence is not returned and the stale cursor cannot silently continue

#### Scenario: Stale correction
- **WHEN** a memory editor saves an obsolete expected revision
- **THEN** the system rejects the write and the web client preserves the user's draft
