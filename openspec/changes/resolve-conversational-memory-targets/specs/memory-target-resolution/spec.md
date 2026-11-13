## ADDED Requirements

### Requirement: A literal miss may select from complete sealed target candidates
The system SHALL preserve the validated current-request action and fact, enumerate
a complete bounded account-scoped effective candidate set, and let a separate
isolated model return only a candidate index or fixed clarification. Unique literal
matches SHALL remain a fast path; ambiguous literal matches SHALL NOT use selection.

#### Scenario: Broader category wording identifies one assertion
- **WHEN** a current correction or forget request has no literal target match
- **THEN** a separate isolated call may select one listed assertion without supplying a tenant, memory ID, action or fact

#### Scenario: Candidate text contains instructions or meaningful qualifiers
- **WHEN** candidates contain role claims, commands, negation, other people or scoped assertions
- **THEN** candidates remain data and cannot expand command authority; the selector is instructed to preserve scope and clarify ambiguity, while semantic accuracy is evaluated separately

#### Scenario: Enumeration cannot cover the account scope
- **WHEN** candidate row or byte bounds would truncate the set
- **THEN** no semantic target selection occurs and the command requests narrower wording without mutation

### Requirement: Candidate selection remains pinned across dispatch and commit
The system SHALL seal the original intent before selection dispatch, preserve the
same namespace across retry, and recheck knowledge generation, complete candidate
snapshot and separate effective/head revisions before selection and mutation.

#### Scenario: Candidate or effective time changes
- **WHEN** any pinned candidate, generation, effective revision, head or uniqueness context changes
- **THEN** an eligible command clarifies without mutation or selection under the new state; revoked or purged executions keep their existing failure behavior

#### Scenario: A selection round is interrupted or purged
- **WHEN** the user explicitly retries an interrupted selection
- **THEN** the same intent and candidates resume without intent reclassification; a purged plan cannot be resurrected

#### Scenario: Selection is malformed or authority has expired
- **WHEN** selection provides extra authority fields, an unlisted index, multiple calls, or an invalid execution/session/provider lease
- **THEN** no memory mutation occurs

#### Scenario: A candidate is forgotten or its source withdrawn before selection
- **WHEN** any enumerated candidate is forgotten or its evidence is withdrawn, including an unselected candidate
- **THEN** dependency invalidation purges the private plan and checkpoints immediately and no resumed selector dispatch can retransmit that candidate

#### Scenario: Retry reset or time drift would expose a stale snapshot
- **WHEN** retry updates execution generation or effective time changes while the sealed snapshot remains older
- **THEN** resume and model-step admission recheck the sealed snapshot and refuse a new selector request without exposing its old candidate data
