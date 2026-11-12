## ADDED Requirements

### Requirement: Recipes are discovered from actual run capabilities
The system SHALL provide compact metadata for available bundled procedures and load
bounded instructions only by an exact pinned identity/revision/content digest.

#### Scenario: User asks for a task without naming a skill
- **WHEN** a tool-capable run requests a briefing, meeting preparation or evidence comparison
- **THEN** the model can load the relevant procedure and use existing scoped knowledge tools without another general task-classifier call

#### Scenario: Unknown or unavailable recipe revision
- **WHEN** a requested recipe is absent from the admitted profile or its exact bundled bytes are unavailable
- **THEN** the tool returns a repairable error and never silently substitutes a different revision

### Requirement: Tool profiles remain stable through retries and upgrades
The system SHALL pin its private tool/recipe profile at admission and preserve it across
attempts without changing the public model-choice DTO or giving legacy runs new tools.

#### Scenario: Legacy run has no profile
- **WHEN** an older run is claimed or retried after the update
- **THEN** it retains the existing knowledge-only tool vocabulary and cannot invoke the new tools

#### Scenario: Recipe output is followed by a retry
- **WHEN** a run has executed a skill tool before interruption
- **THEN** retry keeps the same profile and cumulative budgets and does not regain memory-planning authority

### Requirement: Arithmetic is exact and bounded
The system SHALL compute supported sum, difference and product operations on bounded
decimal strings with checked integer arithmetic and report invalid/overflow inputs.

#### Scenario: Comparison includes fractional prices
- **WHEN** valid decimal inputs require quantity multiplication and total addition
- **THEN** the result is exact without floating-point rounding and is persisted in the existing tool slot

#### Scenario: Input requests unsupported evaluation
- **WHEN** arguments contain code, expressions, exponent notation or exceed supported bounds
- **THEN** no code executes and the caller receives a persisted repairable tool error

### Requirement: Task output preserves evidence and honest completion
The system SHALL retain scoped evidence/revocation semantics for skill-assisted results
and describe unsupported external work as unavailable rather than completed.

#### Scenario: A comparison source is deleted
- **WHEN** a source used by a skill-assisted comparison is removed
- **THEN** its derived answer, evidence and private execution payloads are revoked by the existing dependency rules

#### Scenario: Price or external action is missing
- **WHEN** a cost is unknown or the user requests a connector not available to the run
- **THEN** the procedure preserves the missing information and cannot use instruction text as proof of purchase, reservation, sending or live tracking

### Requirement: Quality reports separate synthetic and live evidence
The system's acceptance record SHALL distinguish deterministic runtime checks, real-model
task outcomes, repeated development cases and unevaluated cases with model/effort/transport provenance.

#### Scenario: A live case fails after successful tool transport
- **WHEN** the model chooses the wrong procedure, misreads evidence or reports unsupported completion
- **THEN** the case remains a task failure rather than a transport success or a silently replaced example
