## ADDED Requirements

### Requirement: Verified task follow-up proposals

The assistant SHALL offer a bounded follow-up after an achieved one-off task with
current verified synchronized-source receipts. A proposal SHALL grant nothing.

#### Scenario: Completed sourced task

- **WHEN** a task verifies its result against actual current source reads
- **THEN** one idempotent proposal SHALL retain the original task and acceptance,
  infer only exact source identities or one committed connected-thread identity,
  and explain whether future synchronized replies are included.

### Requirement: Scoped reusable monitoring permission

The owner SHALL approve the concrete read/check/notify scope, cadence and expiry.
Optional reuse SHALL match the same account, epoch, resource scope and check
template. It SHALL NOT authorize external effects or mail sending.

#### Scenario: Matching later completed task

- **WHEN** a later proposal matches an active reusable permission
- **THEN** it SHALL reuse the active monitoring schedule without duplicate child
  work or silently extending cadence, expiry or scope.

#### Scenario: Permission was limited to one task

- **WHEN** a later task matches a non-reusable scope
- **THEN** it SHALL remain a proposal until explicit acceptance, even if an
  existing monitor can be coalesced after that decision.

### Requirement: Change-gated bounded execution

The scheduler SHALL inspect bounded source metadata before model admission and
atomically record the admitted fingerprint and child identity.

#### Scenario: Idle, unrelated or version-only update

- **WHEN** relevant material source contents are unchanged, another thread changes,
  or only a provider version changes
- **THEN** polling SHALL create no child mission or model call.

#### Scenario: A relevant reply arrives

- **WHEN** the approved synchronized thread gains a current relevant message
- **THEN** one bounded child SHALL read changed members only, retain exact source
  receipts, use only permitted source reads and arithmetic, and avoid replaying
  unchanged failed or unknown work. Coverage limits SHALL be explicit.

### Requirement: Current permission at delivery

Monitor dispatch, result publication, notification reads and queued Push delivery
SHALL require current owner permission and resource scope. Ordinary source updates
SHALL remain distinguishable from withdrawal of the original authority.

#### Scenario: Permission changes after publication

- **WHEN** permission expires, the parent is deleted, or the connection is withdrawn
  after a notice was published and before delivery or reading
- **THEN** the notice SHALL be withheld without waiting for the next cadence.

#### Scenario: Owner resolves an observation

- **WHEN** the owner marks one observation reviewed and optionally corrects it
- **THEN** that observation SHALL not notify again, and the note SHALL stay local
  to this monitor without broadening permission or becoming global memory.
