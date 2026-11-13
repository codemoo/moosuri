## ADDED Requirements

### Requirement: One durable direct adoption boundary
Covered knowledge, command-forget, thread-delete and mail-disconnect operations
SHALL append and fsync exactly one bounded successor record before their complete
effects, applicable receipts and successor watermark commit together. Records
SHALL contain opaque direct tenant/object identifiers and needed revisions/epochs,
not plaintext content, remote identifiers or credentials.

#### Scenario: Effect transaction fails after adoption
- **WHEN** a fsynced record exists but its DB transaction aborts
- **THEN** the actor rejects further work until reopen replays it before readiness
- **AND** unrelated tenant data remains unchanged

### Requirement: Bounded atomic source page withdrawal intent
A source batch SHALL use one record with at most32 direct source UUIDs and SHALL
validate all page mutations before adoption. Replay SHALL withdraw sources without
inventing an uncommitted page receipt or advancing its cursor.

#### Scenario: Mixed page interrupted before receipt commit
- **WHEN** a mixed upsert/withdrawal page fails after intent fsync
- **THEN** replay preserves withdrawals while upserts and receipt/cursor remain uncommitted
- **AND** retry with the same business UUID and renewed lease can converge without resurrection

### Requirement: Frozen predecessor and migration coverage
Migration0015 SHALL bind the successor to the installation UUID and exact frozen
legacy terminal sequence/head after durable header publication. Startup SHALL
verify and replay the legacy prefix before successor effects and readiness.
Missing, partial or mismatched installed journals SHALL fail without reset.

#### Scenario: Migration DB commit is interrupted
- **WHEN** a valid header exists but the successor schema was not adopted
- **THEN** retry reuses the exact header and rejects a partial or conflicting file

### Requirement: Explicit export format compatibility
Schema0015 exports SHALL use format2 with exactly five fixed entries and required
coverage version/baseline, predecessor boundary and successor complete/applied
heads plus byte hash/size. Format1 SHALL retain its original four-entry codec.

#### Scenario: Successor journal or coverage is omitted
- **WHEN** a format2 manifest or source set omits a mandatory field or journal
- **THEN** export or manifest decoding fails without a downgrade or invented file
- **AND** no historical restore or independently latest checkpoint is claimed
