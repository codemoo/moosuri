## ADDED Requirements

### Requirement: Deterministic public lock inventory
The Rust development tool SHALL emit CycloneDX1.6 JSON for all committed Rust,
web, vault-web and OpenSpec lock dependencies, preserving different versions,
package identity and lock digests without network access or dependency installation.

#### Scenario: Repeated and nested dependencies
- **WHEN** the same npm version appears in multiple public lock locations
- **THEN** the inventory SHALL merge its locations and retain other versions separately.
- **AND** identical locks SHALL produce identical bytes without time or host paths.

### Requirement: Explicit provenance limits
The inventory SHALL identify its full source-lock scope and SHALL NOT assert binary
composition, license clearance or vulnerability clearance. Private resolved URLs,
credentials and unsupported Rust source addresses SHALL NOT be exported.

#### Scenario: Unsupported source or corrupt lock
- **WHEN** a lock contains an unsupported source, traversal key or invalid format
- **THEN** generation SHALL fail before output using a diagnostic that excludes source values.
