## ADDED Requirements

### Requirement: Explicit offline dependency evidence
The release tool SHALL collect declared Rust dependency licenses and candidate notice text from an explicitly selected stable vendor tree, preserving exact versions and checksums without network access or code execution.

#### Scenario: Deterministic attribution collection
- **WHEN** the committed lock and matching public vendored sources are supplied
- **THEN** repeated collection produces identical ordered evidence with exact notice text and no host paths

### Requirement: Missing or changed evidence
The tool SHALL reject unsupported sources, unsafe relative paths, observed symlinks and mismatched checksums, and SHALL identify missing declarations/notices without inferring terms. Optional strict mode SHALL fail after emitting an incomplete report.

#### Scenario: Incomplete dependency notices
- **WHEN** a locked package has no selected notice file
- **THEN** its missing status is explicit and strict mode returns failure

### Requirement: Honest release scope
The report SHALL distinguish source-lock evidence from legal clearance, registry authentication, minimal binary composition, npm coverage and signed release acceptance.

#### Scenario: Source-lock report generated
- **WHEN** notice generation succeeds
- **THEN** the report still states its source trust and release-coverage limits
