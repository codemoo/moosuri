## ADDED Requirements

### Requirement: Explicit immutable owner acceptance revisions
The system SHALL accept a task/constraints/checks revision only from the authenticated owner of a stable paused one-off mission, using current revision CAS and idempotent request identity. It SHALL preserve the original and immutable criteria history and SHALL distinguish an edit from additive steering and budget extension.

#### Scenario: Lost acknowledgement and reopen
- **WHEN** an owner saves revised checks and repeats the exact UUID/payload after reopen
- **THEN** the same criteria revision is returned without another edit or execution grant
- **AND** changing that UUID payload conflicts while draft inputs remain visible after an uncertain response.

### Requirement: Revised criteria fence the old plan and candidate
Plans, acknowledged rounds and verification SHALL be pinned to the active acceptance revision. Superseded steps SHALL NOT authorize new IO, publish a late result, block the fresh plan or satisfy new checks.

#### Scenario: Stable local tools were planned before editing
- **WHEN** an owner changes acceptance before old local tools enter
- **THEN** their immutable slots remain audit data but cannot execute, block a new step or be re-leased under the new revision.

#### Scenario: Actual plan and bounded repair
- **WHEN** a revised mission replans, executes current tools and proposes an incorrect result
- **THEN** verification uses its revised checks and repairs within the original remaining model/tool/time and maximum two-repair limits.

### Requirement: Revision does not expand authority or revive withdrawn work
Revision SHALL retain the original capabilities, model, connected accounts, session/epoch authority, expiry, spent calls and repair count. Recurring templates/occurrences and uncertain/expired/restored/withdrawn work SHALL NOT be edited through this path.

#### Scenario: New connected or unsupported execution is required
- **WHEN** an edit requests an unadmitted connected domain, an unavailable explicit public-web read or a recognized unsupported effect/mode
- **THEN** the save rejects it without silently dropping that requirement or widening a grant.

#### Scenario: Destructive lifetime
- **WHEN** a source is withdrawn, a mission/account is deleted or a sealed snapshot is restored
- **THEN** existing revocation/quarantine applies; private acceptance state is purged on deletion and history alone cannot reactivate execution.
