## ADDED Requirements

### Requirement: Stopped predecessor-schema upgrade
A local bundle update SHALL support the target executable's exact Store schema and immediately preceding schema. It SHALL retain the existing pinned signature, target, exact binary/hash/probe, increasing sequence and nondecreasing SemVer checks. The signed old release's schema SHALL equal the captured stopped source schema. Cross-schema migration SHALL require the executing target updater's bytes to equal the verified staged signed target executable.

#### Scenario: Supported predecessor with private content
- **WHEN** the operator supplies a matching signed target bundle and independently pinned unchanged public key for the immediately preceding schema
- **THEN** runtime-exclusive admission and the same original writer lock SHALL cover verified stopped source export, real Store migration and activation
- **AND** source-schema export metadata, source user content and existing private files SHALL be preserved
- **AND** no service or network configuration SHALL execute.

#### Scenario: Unsupported or active source
- **WHEN** the schema has a gap, exceeds the target, differs from the signed old release, the key/target/binary differs, or a managed/data writer is active
- **THEN** migration and activation SHALL reject without a successful upgrade receipt.

### Requirement: Durable forward recovery and manager activation
Before migration the installer SHALL durably record old/new releases, source/target schemas and source export receipt, then durably mark migration started before invoking Store. Migration SHALL use the product Store implementation without copying SQL. Managed startup SHALL refuse a pending activation. Target manager refresh, managed digest update and current-pointer activation SHALL be recoverable from each interrupted publication window.

#### Scenario: Interrupted migration or manager publication
- **WHEN** interruption occurs before/after Store migration or manager/current-pointer publication
- **THEN** the same signed target bundle executable SHALL remain an explicit stopped recovery entry point even while the legacy installed manager cannot read the new metadata
- **AND** choose-new SHALL retry migration and activation idempotently, preserving already committed user work and authority.

#### Scenario: Attempted older selection
- **WHEN** a cross-schema migration-start marker exists or the stopped database no longer has the recorded source schema
- **THEN** choose-old SHALL reject, even if an early migration transaction failed
- **AND** no recovery path SHALL copy an old export back, downgrade SQLite or replay committed work.
- **WHEN** a same-schema pending activation is recovered, or a cross-schema prepared record has not started migration and still has source schema
- **THEN** choose-old MAY retain the old binary selection without changing SQLite.

#### Scenario: Interrupted initial pending write
- **WHEN** writing the initial pre-migration pending record fails or stops before publication
- **THEN** only a private temporary file MAY remain and no truncated public pending marker SHALL be visible
- **AND** the old selected binary SHALL remain startable, while update MAY retry with a fresh export output without overwriting any existing pending record.
