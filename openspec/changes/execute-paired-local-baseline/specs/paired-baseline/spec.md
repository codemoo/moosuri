## ADDED Requirements

### Requirement: Isolated unchanged local baseline
The evaluator SHALL execute the unchanged content-pinned private subject only from
a newly rechecked allowlisted local capsule, with fresh private PG16 over a Unix
socket, cleared synthetic-only configuration and enforced child filesystem/egress
boundaries. Dependency preparation SHALL be separate from subject execution.

#### Scenario: Missing isolation or mismatched source
- **WHEN** a capsule pin, OS sandbox denial probe or required local runtime fails
- **THEN** the two-world denominator retains its failed/not-run outcome
- **AND** no original service, DB, configuration or provider is used as fallback.

#### Scenario: Original or unprepared matching source
- **WHEN** an original checkout or unprepared directory matches the historical source pin
- **THEN** execution still rejects before profile creation or child admission
- **AND** only a canonical current-owner0700 direct tmp capsule with copy/preparation markers and matching prepared runtime can execute.

### Requirement: Two common observable outcomes
The paired suite SHALL compare only timeless memory read/graceful reopen and current
user-input arithmetic through actual runtime/store paths with independent grading.

#### Scenario: Prose claims success without evidence
- **WHEN** current memory-read evidence is missing or arithmetic differs from the independent scaled-integer result
- **THEN** the outcome fails even when terminal output claims success.

#### Scenario: Public metadata report
- **WHEN** the two scripted protocol adapters finish or fail
- **THEN** a new private report preserves planned/executed/fail/not-run counts and content-free evidence
- **AND** it reports no real-model or non-inferiority acceptance.

#### Scenario: Observation fails after actual admission
- **WHEN** a backend has admitted a real request but observation fails or times out
- **THEN** its executed flag stays true and the world remains executed/fail, separately from attempt and observation-returned flags.
