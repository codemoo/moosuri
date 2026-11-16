## ADDED Requirements

### Requirement: Shared ordered assistant migration SQL

Normal startup and encrypted cutover restore SHALL select assistant migrations
from one complete ordered version/SQL registry. Selection SHALL reject missing,
duplicate or unordered entries and schemas outside the caller's explicit
supported range. Schema73 and all existing migration SQL bytes SHALL remain
unchanged by the consolidation.

#### Scenario: Later migration is registered once

- **WHEN** a supported predecessor is upgraded through startup or restore
- **THEN** both routes select every later registered migration through current
- **AND** an SQL file omitted from the registry fails the directory coverage test

#### Scenario: Caller contracts remain distinct

- **WHEN** startup upgrades a pre-schema15 database
- **THEN** journal durability and metadata adoption retain their existing order
- **AND** restore retains its sealed schema19 floor, single quarantine transaction,
  FK/trusted-schema policy, enrollment quarantine and publication validation

#### Scenario: Invalid registry or unsupported predecessor

- **WHEN** selection finds a missing/duplicate/unordered version or unsupported schema
- **THEN** selection fails before any selected SQL is executed
