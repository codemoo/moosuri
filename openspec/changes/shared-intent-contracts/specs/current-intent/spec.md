## ADDED Requirements

### Requirement: Finite current-intent vocabularies agree with admission

Current-user memory, artifact and Calendar planner schemas MUST derive finite
codes from shared typed/core vocabularies used by parsing or Store admission.
Calendar clarification MUST use a finite non-null machine reason for a new clarify
proposal, rather than model-written explanation prose. Existing quote, owner,
account, civil-time, target and effect authority checks MUST remain in force.

#### Scenario: Missing Calendar duration remains the same pending action

- **WHEN** a current request names an event title, date and start but lacks duration
- **THEN** the parser and Store accept a complete duration clarification proposal
- **AND** the next literal answer continues that same initiating request and draft
- **AND** one completed explicit action causes exactly one counted typed Calendar write

#### Scenario: Schema-valid prose cannot stand in for a machine reason

- **WHEN** a new Calendar proposal supplies prose, blank or null clarification for clarify
- **THEN** current-model parsing and new Store admission reject it without recording an action
- **AND** no reason, date, duration or authorization is guessed from that value

### Requirement: Complete current-model objects preserve explicit null semantics

Current-model Calendar and artifact proposals MUST include all advertised keys;
missing nullable keys, unknown codes, duplicate keys and invalid non-command command
fields MUST NOT be normalized into a valid write plan. Persisted Calendar DTO/replay
compatibility MUST retain its existing defaults and clarification fallback.

#### Scenario: Artifact model omits a required null field

- **WHEN** an artifact classifier omits target_quote from its command object
- **THEN** the runtime fails the malformed planning round before another inference or write
- **AND** no artifact receipt or stored artifact is created

#### Scenario: Persisted legacy Calendar clarification

- **WHEN** an already stored Calendar decision contains a former freeform reason or omitted legacy field
- **THEN** its existing replay decoding/validation and generic message fallback remain available
- **AND** that compatibility does not admit the same malformed new model proposal
