## ADDED Requirements

### Requirement: Shared dispatch preserves domain authority
Each isolated current-request planner SHALL use one shared transport path while
retaining its own closed parser and Store admission. Remote inference and an
ambiguous dispatch acknowledgment MUST NOT be retried; only the same local
checkpoint may use the existing bounded transient retry.

#### Scenario: Domain planner rejects a malformed model decision
- **WHEN** a completed planner response fails its domain parser
- **THEN** no ordinary domain tool or mutation SHALL execute from that decision
- **AND** its recorded dispatch SHALL remain accounted for

#### Scenario: Mission chat uses typed admission
- **WHEN** the shared dispatcher runs a mission/Calendar classifier
- **THEN** it SHALL use mission-chat admission while artifact/memory use their existing local-intent admission
- **AND** current request isolation and original budget SHALL remain unchanged

### Requirement: Mission rounds reuse only the currently authorized transport
Personal mission rounds SHALL resolve current lease and token readiness before
reusing that run's transport. Rotation MUST replace its authorization; cancellation
and withdrawal MUST continue to prevent later inference.

#### Scenario: Multiple successful rounds in one personal mission
- **WHEN** an admitted mission reads its source, calculates and completes through successive model rounds
- **THEN** requests SHALL reuse the established HTTP connection and persist actual evidence
- **AND** readiness checks SHALL still run for every new dispatch
