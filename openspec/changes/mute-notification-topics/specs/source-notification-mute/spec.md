## ADDED Requirements

### Requirement: Owned stable source delivery preference
The system SHALL let the authenticated current owner mute/unmute a fixed reminder
or recurring mission schedule by its stable source ID and independent expected mute
revision, without changing execution authority or using keywords to infer a topic.
It SHALL expose the owned source on existing inbox cards and a complete bounded
muted-source list on that same page, including removed and paused schedules.

#### Scenario: Another account or stale setting attempts a change
- **WHEN** an unknown source, another owner's source, revoked session or stale preference revision is submitted
- **THEN** the change is rejected without changing any owner's delivery or execution state
- **AND** late browser responses cannot update a different account or a newer request

### Requirement: Immutable suppressed publication and preserved history
The system SHALL snapshot source suppression in the actual publication transaction,
retain actual occurrence/task receipts and history, and omit suppressed publications
from every inbox page permanently. Read/dismiss and existing inbox entries SHALL
remain unchanged by mute/unmute. Preferences SHALL survive restart and restore
without resuming quarantined execution or Push authority.

#### Scenario: Work completes while muted then the owner unmutes
- **WHEN** reminders or recurring tasks publish while their source is muted and the owner later unmutes
- **THEN** the recorded work remains in history but withheld notices never replay in inbox or Push
- **AND** future unmuted publications may notify normally under the existing fences

### Requirement: Ordered Push suppression without recall claims
The system SHALL atomically latch Push suppression on pre-existing source notices
when muted, terminalize queued/claimed work and recheck the current preference and
latch before dispatch and passive display authorization. Unmute SHALL never clear
notification latches or revive retries. Already dispatched results SHALL be recorded
honestly and SHALL not be undone or resent by mute/unmute.

#### Scenario: Mute races held preparation or queue scanning
- **WHEN** mute commits before a claimed dispatch or before an old notification is scanned
- **THEN** neither notice can dispatch after unmute and unrelated sources/owners are unaffected

#### Scenario: Dispatch commits before mute
- **WHEN** external dispatch authority is committed first
- **THEN** its actual receipt remains and mute cannot promise recall
- **AND** later passive authorization and retry reject the latched notice
