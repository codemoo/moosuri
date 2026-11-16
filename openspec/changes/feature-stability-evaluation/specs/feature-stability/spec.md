## ADDED Requirements

### Requirement: Feature-scoped contract evidence
The evaluator SHALL preserve the existing complete suite by default and allow a
compiled feature subset without certifying excluded features or losing failures.

#### Scenario: Memory-only run
- **WHEN** the caller selects the memory feature
- **THEN** only its existing worlds execute, applicable and common controls remain,
  and the report records every excluded world and unselected feature.

#### Scenario: Incomplete evidence
- **WHEN** an expected row is missing, failed or not run, or a required control is absent or fails
- **THEN** the feature cannot pass and its expected case denominator remains visible.

#### Scenario: Default execution
- **WHEN** no feature is selected
- **THEN** all existing worlds and required controls execute, and the report records
  complete selection and feature summaries without changing task/handling eligibility.

### Requirement: One unfinished child per recurring schedule
A recurring schedule SHALL retain at most one unfinished child and one latest
queued occurrence, including while its child waits for user input.

#### Scenario: Waiting child with another occurrence due
- **WHEN** a child is waiting and more occurrences become due
- **THEN** no second child starts for that schedule, only its latest queued
  occurrence is retained, and unrelated due schedules remain eligible.

#### Scenario: Child settles
- **WHEN** the unfinished child is cancelled or reaches a terminal result
- **THEN** the latest eligible queued occurrence can start once with its original
  due-based deadline, and normal future occurrences remain eligible.
