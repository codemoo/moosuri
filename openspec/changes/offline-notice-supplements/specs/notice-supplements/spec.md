## ADDED Requirements

### Requirement: Explicit bounded offline notice supplement

Rust and web notice collection SHALL accept a shared optional offline manifest binding exact ecosystem/name/version and current lock checksum/SRI to explicit reviewed repository/full commit/source paths, provenance kind and exact UTF-8 notice text/hash. It SHALL reject unsafe paths, symlinks, duplicate/unknown fields or entries, mismatched identities/hashes and size violations without installing, fetching or executing package code.

#### Scenario: Matching reviewed source notice

- **WHEN** every supplied entry matches current lock identity/digest and its selected text hash
- **THEN** matching components and separately counted coverage retain that supplemental text/provenance deterministically without claiming source or legal authenticity.

#### Scenario: Unsupported or changed input

- **WHEN** an entry is unknown, duplicated, unsafe, malformed, oversized or hash-mismatched
- **THEN** the collector fails with a bounded fixed error before emitting supplemental evidence.

### Requirement: Original completeness remains unchanged

Supplemental evidence SHALL preserve all original declaration, missing, referenced-notice and conflict fields and SHALL NOT change strict completeness or the signed release format.

#### Scenario: Locally missing or conflicting evidence with a supplement

- **WHEN** a local component remains incomplete and external notice text is supplied
- **THEN** the report retains the local gap, counts supplemental coverage separately and `--require-complete` still emits its report then fails.

#### Scenario: No optional manifest

- **WHEN** the caller supplies no supplement
- **THEN** existing report and completeness behavior remain unchanged.
