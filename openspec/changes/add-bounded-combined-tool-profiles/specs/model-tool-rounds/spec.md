## ADDED Requirements

### Requirement: Bounded combined tool definitions reach the model intact

The model protocol SHALL preserve up to 32 validated definitions for shared dialects
and the personal ChatGPT plan namespace. It SHALL reject larger lists before provider
IO without truncation. Schema-byte, name, capability and execution limits SHALL remain
enforced.

#### Scenario: Combined production definitions complete a plan or shared round trip

- **WHEN** version5 knowledge/recipe/decimal, Gmail, HTTP/search/browser, read-only artifact and Calendar builders produce21 definitions
- **THEN** an owned plan or shared-compatible HTTP provider observes all21 exact definitions and account pins
- **AND** a completed decimal call, explicit synthetic result and final continuation validate without a preflight definition-count failure
- **AND** this transport scenario does not claim Store admission or external adapter execution.

#### Scenario: Definition count remains bounded

- **WHEN** a caller supplies32 individually valid definitions within all byte limits
- **THEN** all definitions are encoded in every supported dialect
- **WHEN** a caller supplies33 definitions
- **THEN** validation rejects the request rather than dropping a definition.

#### Scenario: Larger vocabularies retain size and name fences

- **WHEN** individually valid definitions exceed64KiB aggregate schemas/descriptions
- **THEN** the request is rejected
- **WHEN** a completed model call names a function absent from the admitted definitions
- **THEN** the round is rejected even when the admitted list contains32 definitions.
