## ADDED Requirements

### Requirement: Answer-backed completion

The assistant SHALL require a nonempty answer body rather than commentary alone
before treating an ordinary completed model response as a completed answer.

#### Scenario: Provider finishes after progress text

- **WHEN** a provider reports completion with only commentary and no tool calls
- **THEN** the stored run SHALL remain incomplete with a protocol failure while
  preserving the commentary for display.

#### Scenario: Phase metadata arrives after body text

- **WHEN** a response item's phase arrives after its body
- **THEN** terminal classification SHALL use that final metadata; final or
  unphased answer text SHALL still qualify for completion.

### Requirement: Completed native results retain final-answer capacity

The assistant SHALL permit a completed native result to resume within its
remaining model budget even when the last permitted tool slot was consumed.

#### Scenario: Twelfth tool result is ready

- **WHEN** the current native read completes at tool slot 12 and a model dispatch
  remains within the original deadline and authority
- **THEN** the same run SHALL be able to produce its final answer without resetting
  its counters or deadline, and tool slot 13 SHALL remain forbidden.

#### Scenario: Model budget is exhausted

- **WHEN** all six remote model dispatches were consumed before native completion
- **THEN** resumption SHALL NOT allocate another model dispatch.
