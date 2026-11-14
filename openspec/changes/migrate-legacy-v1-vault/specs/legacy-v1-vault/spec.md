## ADDED Requirements

### Requirement: Explicit authenticated legacy export
The client SHALL accept the documented old `musuri-vault` v1 authenticated export only through an explicit user-local legacy route and SHALL authenticate the entire manifest and every payload before preview.

#### Scenario: Password or recovery migration
- **WHEN** an unlocked current v2 owner selects a supported old export and its export-time password or MR1 recovery key
- **THEN** local authentication returns ordinal duplicate/new/changed statuses and selected items are freshly encrypted with new IDs under that current owner's v2 binding
- **AND** source archive, plaintext and unlock secrets are absent from network requests.

#### Scenario: Incompatible or tampered file
- **WHEN** a file lacks a manifest, exceeds256 items or26MiB, uses unsupported KDF parameters, has duplicate/unknown metadata or invalid canonical identities, or fails any manifest/payload authentication
- **THEN** the client returns no preview or saved source items
- **AND** structural/size/KDF-policy failures occur before source KDF and input is never truncated.

#### Scenario: No format or completeness fallback
- **WHEN** old v1 is supplied to the ordinary current archive route or a raw header/items dump is supplied to the legacy route
- **THEN** it is rejected without a fallback
- **AND** a supported export authenticates only its supplied inventory, without claiming source-server completeness/latestness.

### Requirement: Existing destination commit semantics
The client SHALL keep current import selection, immutable ciphertext retry, owner isolation and cancellation semantics for migrated rows.

#### Scenario: Lost acknowledgement and interruption
- **WHEN** a migrated destination item loses its publication acknowledgement or the user cancels
- **THEN** reconciliation retains the exact original operation until an exact receipt confirms it and cancellation preserves committed items while disposing remaining staging.

#### Scenario: Destination changes or locks
- **WHEN** the current vault locks, account changes, or its key generation changes during a source-file read or migration
- **THEN** late work cannot install preview or import state into the changed destination.
