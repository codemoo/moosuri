## ADDED Requirements

### Requirement: One guarded committed-round lifecycle
Fresh and reopened supported missions MUST consume an acknowledged round through
the same guidance, plan, tool, owner-input and final-outcome handling. Existing
terminal admission, budgets, current authority and receipt checks MUST remain in
force. Saved successful tool work MUST NOT execute again.

#### Scenario: Typed final fails local validation
- **WHEN** an admitted final has blank text and Store rejects it
- **THEN** fresh execution and recovery settle with the existing protocol-failure mapping
- **AND** no verified candidate or additional inference is produced

#### Scenario: Legacy plan fails local validation
- **WHEN** a verified recovered planning capsule contains an empty plan
- **THEN** the mission settles protocol failure instead of remaining active
- **AND** no new model dispatch is required

#### Scenario: Recovered malformed guided response
- **WHEN** a recovered empty-call response is malformed and the exact step owns received guidance
- **THEN** it settles protocol failure before applying that guidance
- **AND** terminal closure records the receipt without claiming application

#### Scenario: Guidance supersedes an owner-input proposal
- **WHEN** new current-attempt guidance is accepted before input-wait admission completes
- **THEN** both execution paths recheck its current receipt before treating rejection as a protocol failure
- **AND** the new guidance remains available to the next admitted dispatch

#### Scenario: Authority or acceptance changes
- **WHEN** Store fences a stale round after owner/epoch/cancel/acceptance change
- **THEN** the old round stops through the existing guard
- **AND** it cannot overwrite the new state, execute effects or grant new rights

### Requirement: Exact step guidance read
Store MUST guard the step lease before reporting whether its exact current
execution attempt and dispatch step own a received instruction. Unassigned,
applied, foreign and stale-step receipts MUST NOT authorize acknowledgement.

#### Scenario: Assignment and application
- **WHEN** guidance arrives after one step begins and a later dispatch assigns it
- **THEN** the earlier step does not own it and the assigned current step does
- **AND** application removes it from the received-step read
