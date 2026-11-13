## ADDED Requirements

### Requirement: Current-user conversational fixed reminders
The Store SHALL accept a clear bounded current-user reminder request without a
redundant approval step and SHALL atomically save its schedule effect, stable
receipt, fixed confirmation and completed chat. Only authenticated current user
text MAY grant this mutation; retrieved/model/skill/mail text SHALL NOT grant it.

#### Scenario: Lost acknowledgement and restart
- **WHEN** the same UUID and exact text/timezone/selected target are retried
- **THEN** the original completed chat and schedule receipt return without a duplicate effect
- **AND** changing any bound payload component conflicts

#### Scenario: Failed confirmation
- **WHEN** confirmation persistence fails after an attempted schedule mutation
- **THEN** no schedule effect, receipt or partial admitted chat commits

#### Scenario: Target and authority changed
- **WHEN** a selected revision is stale, a target belongs to another account, or the current session was revoked before admission
- **THEN** no mutation commits and no model/provider is invoked for the rejected command

#### Scenario: Ordinary text or unsupported mission
- **WHEN** an ordinary, negated, translated or quoted request is received
- **THEN** it cannot mutate schedules through conversational capture
- **AND** scheduled research/send missions receive an honest unsupported result without a false fixed-reminder mission claim

### Requirement: Weekly local recurrence
The scheduler SHALL retain sorted unique ISO weekdays1..7, wall time and IANA zone,
reuse gap skips and first-fold execution and coalesce downtime into bounded
receipts rather than materializing an unbounded backlog.

#### Scenario: DST and missed weekly occurrences
- **WHEN** a selected weekly local slot is absent, repeated or missed during downtime
- **THEN** absent slots skip, repeated slots execute once at the first offset, and only the latest valid missed slot is delivered with older range counts

#### Scenario: Browser retry context
- **WHEN** a send is uncertain and browser timezone or current selection changes
- **THEN** explicit retry retains the original timezone and target revision

### Requirement: Media and restore integration preserve current authority

Conversational reminder admission SHALL preserve immutable image bindings and the original retry payload. Attached or inherited media contexts SHALL produce no reminder mutation. Restored command receipts SHALL remain historical results while restored schedules stay paused.

#### Scenario: Image conversation asks to delete an existing reminder

- **WHEN** a current user selects an owned reminder but the conversation carries an image dependency
- **THEN** the request completes with a no-effect explanation and leaves the reminder revision and state unchanged
- **AND** replaying the same request retains its original image and timezone payload

#### Scenario: Current snapshot includes a conversational reminder

- **WHEN** the exact sealed snapshot is restored into an absent destination
- **THEN** the completed conversation and receipt remain readable after fresh enrollment
- **AND** the reminder remains paused and no previous work or provider call is replayed


### Requirement: Continuous control follows committed reminders

The chat UI SHALL advance a selected reminder only from a matching committed
request receipt, preserve a newer user selection, and keep the original uncertain
request payload immutable. Complete Korean on/off imperatives SHALL use the same
current-user authority and exact target validation as pause/resume.

#### Scenario: Pause then resume after a lost acknowledgement

- **WHEN** the original pause is reconciled as committed and the user next resumes the same selected reminder
- **THEN** the next command uses the committed revision without duplicating the pause
- **AND** a later acknowledgement never overwrites a newer selection

#### Scenario: Everyday off request and conditional on request

- **WHEN** the owner says 알림 꺼줘 for a uniquely resolved reminder
- **THEN** that reminder is paused rather than deleted
- **AND** a question or deferred condition containing 켜줘 does not resume it
