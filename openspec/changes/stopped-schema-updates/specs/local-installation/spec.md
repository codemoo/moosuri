## ADDED Requirements

### Requirement: Stopped supported multi-version schema upgrade
A local bundle update SHALL admit the fixed source schema47 through the signed
target's current schema, retaining signature, key, target, binary/probe, sequence
and SemVer checks. The signed old release SHALL match the captured source schema.
Cross-schema migration SHALL run only the verified signed target executable under
the original stopped writer lock.

#### Scenario: Supported older source with private content
- **WHEN** the operator supplies a matching signed target bundle and independently pinned unchanged public key for an actual supported source schema47 and a target at least49
- **THEN** runtime-exclusive admission and the same original writer lock SHALL cover verified stopped source export, real Store migration and activation
- **AND** source-schema export metadata, source user content and existing private files SHALL be preserved
- **AND** sequence SHALL increase and SemVer SHALL not decrease
- **AND** the running updater bytes SHALL equal the verified staged target
- **AND** older migration SQL alone SHALL NOT establish support below47
- **AND** no service or network configuration SHALL execute.

#### Scenario: Unsupported or active source
- **WHEN** the source schema is below47, exceeds the target, differs from the signed old release, the key/target/binary differs, or a managed/data writer is active
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


#### Scenario: Intermediate schema during forward recovery
- **WHEN** a multi-version upgrade recorded signed source47 and current target but
  the interrupted database has an intermediate schema inside that interval
- **THEN** the pinned signed target updater SHALL resume the real product Store
  migrations under the retained original stopped writer lock
- **AND** the migration-start latch SHALL continue rejecting old runtime selection
  without restoring an old DB or overwriting the retained source export.
