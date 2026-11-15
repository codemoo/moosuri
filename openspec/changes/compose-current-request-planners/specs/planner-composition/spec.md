## ADDED Requirements

### Requirement: Planner-owned durable dispatch identity
A current-request mission or Calendar classifier SHALL acquire exactly one typed domain marker atomically with its model dispatch. The marker SHALL bind the exact model step, attempt, receipt nonce and original run lease. Global call counters SHALL remain shared budgets and SHALL NOT establish planner provenance.

#### Scenario: Completed non-command artifact precedes ordinary reading
- **WHEN** a saved-document read enters the artifact classifier and completes NoCommand without tools
- **THEN** its exact completed receipt permits an untouched mission route to pass
- **AND** actual knowledge search/read receipts can ground the final answer without artifact, memory or Calendar effects

#### Scenario: Explicit current task or Calendar command follows NoCommand
- **WHEN** the current user explicitly requests a task or Calendar effect after that isolated non-command stage
- **THEN** the mission-domain classifier dispatches at its own next step within the shared budget
- **AND** recording, preflight and effect application require that exact completed decision receipt and original current-user authority

### Requirement: No replay or inferred legacy authority
A dispatched or unknown mission marker, foreign or stale StepLease, ordinary passed route, arbitrary prior round or ambiguous pre-migration history SHALL NOT acquire new mission or Calendar effects. Completed sealed local confirmation SHALL preserve existing original-authority and exact-receipt retry checks.

#### Scenario: Ambiguous classifier acknowledgement and recovery
- **WHEN** a mission classifier dispatch is unresolved and the run is retried or reopened
- **THEN** another classifier dispatch and skip are rejected
- **AND** earlier classifier accounting remains consumed

#### Scenario: Sealed document write has mixed wording
- **WHEN** the artifact-first stage seals a document write in a mixed current request
- **THEN** the runner retains its separate write stage and never enters Calendar or mission effect planning

#### Scenario: Old completed history has no typed marker
- **WHEN** schema58 opens an older pending plan with completed model history lacking domain provenance
- **THEN** no domain marker is inferred or backfilled
- **AND** the preserved history cannot be treated as fresh effect authorization

#### Scenario: Calendar plan schema opens or restores an older installation
- **WHEN** normal opening or sealed restore migrates the original Calendar plan layout
- **THEN** sealed, committed and finished rows, receipts and identity constraints remain intact
- **AND** new plans can reference their actual step within the existing one-through-six budget without granting old plans new dispatch authority
