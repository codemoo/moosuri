## ADDED Requirements

### Requirement: Isolated natural current-owner interpretation

The system SHALL support current Korean/English paraphrases for one read-only delegated task or control without requiring a DSL. Complete supported requests SHALL apply without a routine approval screen. A cue gate SHALL only route cost, and noncandidate normal chat SHALL add zero mission classification calls. Candidate interpretation SHALL use only current text and trusted admission metadata, never source/history/media instructions or model-supplied identities/grants.

#### Scenario: Complete daily request versus missing Monday clock

- **WHEN** the owner asks to summarize public news every morning at9AM
- **THEN** the completed typed current decision SHALL save the existing recurring mission and return its actual admission receipt.
- **WHEN** a Monday recurring request lacks a clock
- **THEN** the system SHALL ask that missing time without inventing a default or requiring an approval for already clear fields.

#### Scenario: Literal reminder and quoted instructions

- **WHEN** a current explicit reminder shell contains research/send/quote words
- **THEN** its saved body SHALL remain literal fixed content.
- **WHEN** source/forwarded/quoted examples are being explained
- **THEN** the semantic interpreter SHALL not treat their nested commands as independent owner authority.

### Requirement: Atomic acknowledged command plans and real authority

A completed semantic decision SHALL persist atomically with its model step. Apply SHALL recheck the exact source lease/epoch, the exact original admission session digest/authentication epoch, source-free eligibility, original model/read profile and actual owner target/revision. Actual mutation, receipt and final confirmation SHALL commit together under the existing bounds.

#### Scenario: Lost acknowledgement and failed confirmation

- **WHEN** a completed decision is retried after process loss
- **THEN** the same stored decision SHALL resume without a new classifier or changed request/model identity.
- **WHEN** confirmation/receipt commit fails after local mutation
- **THEN** the mission/template/control SHALL roll back rather than leave an unacknowledged duplicate effect.

#### Scenario: Current owner/session or target changed

- **WHEN** logout/epoch loss, foreign owner target or stale selected revision precedes apply
- **THEN** no new authority SHALL be created and the system SHALL refuse or clarify the actual missing identity.

### Requirement: Existing mission semantics and truthful results

The system SHALL use the existing time oracle, pinned read-only scope and original mission budgets. Admission SHALL never claim task completion or invent checker facts. Unsupported required effects SHALL not be silently discarded. Unknown dispatched work SHALL not replay.

#### Scenario: Restore or source conversation deletion

- **WHEN** a sealed snapshot restores
- **THEN** unapplied plans SHALL quarantine and historical receipts SHALL not reauthorize execution.
- **WHEN** the originating chat is deleted after a durable task was committed
- **THEN** the separately saved task SHALL retain its existing authority/lifecycle, while deleted source chat plans cannot newly apply.

### Requirement: Safe observable conversational controls

The UI SHALL freeze original request/zone/typed target through uncertain submission, preserve future drafts and fence previous account/thread responses. Named/selected targets SHALL be actual owned identities. Ambiguous target choices SHALL show current owned cards; selection alone SHALL not be an action grant.

#### Scenario: Late control result follows a newer selection

- **WHEN** an old control's receipt arrives after the owner selects a different target
- **THEN** it SHALL not overwrite that new selection or replay the old action.

#### Scenario: Replacement login cannot revive a pending command

- **WHEN** the original source session is revoked or its authentication epoch changes after a decision is acknowledged
- **THEN** pending-plan planning, acknowledgement, apply and explicit Retry SHALL reject that old capability even after fresh login.
- **WHEN** the authenticated owner sends a fresh current request
- **THEN** the new request MAY independently create its own supported task.

#### Scenario: Explicit civil components and source zone agree

- **WHEN** a proposal changes explicit minutes, adds an unmentioned weekday or changes today/tomorrow/ISO date relative to the frozen anchor
- **THEN** the system SHALL clarify without saving that contradictory schedule.
- **WHEN** the current request explicitly names supported UTC or IANA timezone
- **THEN** that source zone SHALL override browser metadata; unknown/conflicting zones or unsupported offset text SHALL clarify.

#### Scenario: Selected identity conflicts with an explicit name

- **WHEN** a source request names a different owned task from the selected identity, including across task kinds
- **THEN** neither target SHALL change and the response SHALL offer actual current owned cards.
- **WHEN** the exact name and selected identity agree
- **THEN** the current revision SHALL still pass CAS before control.

#### Scenario: Rejected target guard survives source confirmation failure

- **WHEN** a verified exact target guard expires or revokes execution before resume/steer
- **THEN** only those guard transitions SHALL commit before the action transaction, with no partial control writes.
- **WHEN** action/receipt/final confirmation later fails
- **THEN** action writes SHALL roll back while the target guard transition remains durable; a completed response SHALL distinguish expired/revoked target from admission capacity.


### Requirement: Missing named targets never select by owner count
The system SHALL require an exact current owned-name quote or an actual selected target for control, preserving the current revision and owner checks.

#### Scenario: A named proposal loses its target quote
- **WHEN** a control proposal omits the name and no actual target is selected
- **THEN** no task is changed even if the owner has only one task
- **AND** the response offers current target cards for clarification

### Requirement: Civil dates and clocks retain their boundaries
The system SHALL keep an explicit ISO civil date separate from its clock and preserve both fields and an explicit supported source zone.

#### Scenario: Date followed by a clock
- **WHEN** an exact supported date and clock are separated by whitespace in a once request
- **THEN** the stored instant retains the stated date, minute and zone
- **AND** a contradictory minute, date or replacement recurring rule is rejected
