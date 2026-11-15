## ADDED Requirements

### Requirement: Clarification preserves the original task
The system SHALL accept bounded owner context and judgment criteria for the original mission without granting new effect, account, recipient, title or future-template authority. It SHALL preserve the original run, acceptance revision, counters, deadline, model/profile and completed receipts while returning runnable capacity. Semantic context SHALL remain subject to owner review.

#### Scenario: Context arrives after completed work
- **WHEN** a mission requests missing context after completing valid reads or calculations
- **THEN** the original card persists the question across restart without model polling
- **AND** an exact idempotent reply resumes the same run with completed work and original limits
- **AND** text requesting another effect cannot create a new admitted action

### Requirement: Choices bind actual current observations
The system SHALL expose only2–5 real current-acceptance candidates identified by source/event ID, revision/ETag and completed receipt step/call ID. Store SHALL derive labels and reject invented, duplicate, changed or withdrawn candidates. A Calendar choice SHALL select only an existing same-title target permitted by the original date-move authority and retain its exact ETag precondition.

#### Scenario: Two stored sources conflict
- **WHEN** the owner selects a source from actual current receipts
- **THEN** the saved choice and final evidence refer to the selected identity
- **AND** a revision change or withdrawal rejects the old answer without silently choosing another source or restarting reads

#### Scenario: Calendar titles are duplicated
- **WHEN** the owner chooses one of two actually observed same-title events
- **THEN** only its exact observed ID/ETag can enter the original date-move ledger
- **AND** another event or a changed ETag fails before mutation admission

### Requirement: Recurring clarification is occurrence-local
The system SHALL permit context/source waits for the current recurring read-only occurrence and SHALL NOT modify or replay answers into future template authority.

#### Scenario: Next occurrence starts
- **WHEN** an answered occurrence ends and the next scheduled occurrence is admitted
- **THEN** the next occurrence has no inherited answer and the saved template is unchanged
- **AND** revoking the recurring grant retires outstanding questions and notices

### Requirement: Action-needed delivery follows current authority
The system SHALL create one generic inbox/WebPush event per persisted question, deep-link the original card and reuse existing owner/session, mute and quiet-hours rules without creating a schedule. Answered, expired, cancelled or withdrawn questions SHALL fail dispatch/display authorization and disappear from actionable inbox results.

#### Scenario: Reply races push delivery
- **WHEN** a push was claimed before the owner answered
- **THEN** committing dispatch after the answer fails and a passive click cannot reopen it as actionable
- **AND** push content contains no question text and navigation uses a server-authorized mission identity

#### Scenario: Upgrade or restore
- **WHEN** schema69 state is upgraded
- **THEN** notification identities, rowids, preferences and push relationships are preserved
- **AND** restoring a snapshot clears replies/waits and quarantines unfinished authority without replay
