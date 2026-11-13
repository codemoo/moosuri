## ADDED Requirements

### Requirement: Explicit fixed local reminder rules
The system SHALL let a current owner create/preview/edit a fixed-content local
reminder using a future UTC once instant, UTC anchor+n×interval of at least60seconds,
or daily HH:MM in a chosen IANA timezone. It SHALL record its rule, timezone database
version and revision and SHALL NOT imply model-task execution or an external send.

#### Scenario: DST gap, fold and interval anchor
- **WHEN** a daily wall-clock slot is nonexistent or repeated
- **THEN** the nonexistent local slot is skipped without inventing a UTC execution and the repeated slot uses its first UTC offset once
- **AND** UTC intervals preserve their original anchor across late completion

### Requirement: Durable bounded misfire and control fencing
The system SHALL use stable occurrence identities, at most one claimed and one
latest queued occurrence per schedule, bounded polling near5seconds and persisted
owner-fair claims. It SHALL coalesce recurring downtime to the latest valid missed
slot, record older skipped ranges and deliver a late one-shot only while authority/
validity permits. Pause/edit/revoke SHALL fence uncommitted old work; resume/edit
SHALL begin with future slots and SHALL NOT replay paused history.

#### Scenario: Pause or edit wins before publication
- **WHEN** an owner pauses/edits/revokes after claim but before local publication
- **THEN** the old nonce/revision cannot publish a notification
- **AND** already committed notifications and other-account schedules remain unchanged

### Requirement: Atomic local inbox receipts and restart
The system SHALL atomically publish an immutable owner notification and its
occurrence receipt with deduplication. Claim alone SHALL grant no publication
authority. Normal restart SHALL recover only uncommitted local work and SHALL NOT
duplicate committed notifications. Restore/cutover SHALL quarantine grants through
the root activation transaction rather than treat restoration as ordinary restart.

#### Scenario: Failure after inbox insert before receipt update
- **WHEN** the publication transaction fails after inserting the inbox entry
- **THEN** the inbox effect and delivered receipt both roll back
- **AND** reopen can deliver once with a fresh claim while a committed receipt remains idempotent

### Requirement: Account-owned inbox and truthful quiet UI
The system SHALL provide authenticated current-account read/dismiss and quiet-hour
settings, retain inbox entries during quiet hours, fence stale account/refresh
responses and preserve original edit revisions. It SHALL request no Push permission
and SHALL leave model tasks, changes-only observations and WebPush explicitly absent.

#### Scenario: Account change or delayed refresh
- **WHEN** an old-account response or pre-mutation refresh arrives after a newer account or accepted mutation
- **THEN** it cannot replace current form/inbox/control state or bypass account revision checks
