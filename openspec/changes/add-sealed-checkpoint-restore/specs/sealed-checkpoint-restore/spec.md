## ADDED Requirements

### Requirement: Exact authenticated sealed snapshot admission
Restore SHALL accept only a supported sealed checkpoint with a separately trusted
head and matching private age identity, authenticate age through EOF and verify all
fixed canonical regular entries, hashes, identities, schema and complete/applied
journal heads before destination publication.

#### Scenario: Wrong or malformed input
- **WHEN** a required input is missing, truncated, tampered, unsupported, contains an extra/path/link entry or mismatches its trusted head
- **THEN** restore fails without admitting or overwriting a destination and retains all input files

### Requirement: Quarantine before runtime
Restore SHALL commit distinct installation identity, immutable provenance, fresh
account enrollment and all pending work/grant quarantine in the isolated verified
copy before normal Store or workers may admit the new root.

#### Scenario: Old authority cannot resume
- **WHEN** the restored root starts and an account later enrolls locally
- **THEN** old sessions/passwords, suspended status, pending dispatch, source grants, extraction and schedules do not silently reactivate, while committed content/withdrawals/receipts/accounting remain

### Requirement: Durable non-overwriting retry
Restore SHALL publish a completely synced private root by atomic no-replace rename,
sync its parent and recognize only matching immutable provenance for same-input
post-publication retries. Runtime SHALL reject reserved staging paths.

#### Scenario: Process exit before or after publication
- **WHEN** a restore process exits at an extraction, quarantine or publication boundary
- **THEN** an incomplete stage cannot start, an unrelated occupied root remains unchanged and same retained inputs can retry without duplicate installation publication

### Requirement: Nonmutating stale startup admission
Every raw runtime constructor SHALL acquire the current writer lock and freshly
inspect the source seal before chmod, file privatization, journal/SQLite mutation,
including an earlier absent or empty installation decision.

#### Scenario: A delayed opener observes a newly sealed source
- **WHEN** a source is initialized and sealed after an opener's absent-root observation
- **THEN** the delayed opener rejects it without changing its restricted directory mode or original files
