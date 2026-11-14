## ADDED Requirements

### Requirement: Offline lock-bound web notice inventory
The maintainer command SHALL collect deterministic selected metadata and notice evidence for every npm lockv3 installation position of both React apps using only explicit installed roots and fixed source locks.

#### Scenario: Nested, scoped, aliased and repeated package identities
- **WHEN** both reviewed installed trees match their lock name/version identities
- **THEN** each app/position, exact version, declaration, selected notice text and content hash is preserved with deterministic ordering and exact lock hashes
- **AND** package scripts, arbitrary metadata, host paths, registry URLs and environment values are not emitted or executed.

#### Scenario: Missing platform or legal evidence
- **WHEN** a locked package is absent or a declaration/candidate notice/reference is missing or conflicting
- **THEN** the position remains explicitly incomplete rather than being omitted or assigned guessed license text
- **AND** require-complete emits the report before exiting nonzero.

#### Scenario: Unsafe or unexpected sources
- **WHEN** input has path traversal, observed symlinks, an unexpected installed identity, unsupported source metadata or invalid selected text/bounds
- **THEN** collection fails without reading arbitrary paths or emitting a partial report containing unselected data.

### Requirement: Distinguish evidence from release clearance
The command and documentation SHALL distinguish local selected-file evidence from package authentication, minimal binary dependency coverage and legal clearance.

#### Scenario: Source integrity and local content change
- **WHEN** a local candidate notice changes
- **THEN** its content hash changes while the source lock integrity remains recorded separately
- **AND** neither integrity metadata nor zero missing candidates claims legal clearance, complete attribution/source obligations or asset coverage.
