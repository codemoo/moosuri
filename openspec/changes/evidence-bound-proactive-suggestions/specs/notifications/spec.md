## ADDED Requirements

### Requirement: Evidence-bound review suggestions
The system SHALL admit only the fixed followup_review product topic on explicit
save of a current active unmuted followup whose due/review/snooze fields are all
absent. It SHALL provide a literal reason and only a current owned record link.
It SHALL admit at most5 pending or currently visible suggestions per account.
Migration, restore and idle polling SHALL NOT backfill old records or call a model.

#### Scenario: Owner opens a saved-record suggestion
- **WHEN** an eligible followup is explicitly saved and its local publication is due
- **THEN** one suggestion states the absent-date reason and links the current owned record
- **AND** opening it only opens the existing editor, without execution or automatic save

#### Scenario: Full list and capacity release
- **WHEN** 5 pending or visible suggestions exist and another record is saved
- **THEN** the record is saved without admitting another suggestion or overflow queue
- **AND** hiding or invalidating one suggestion releases capacity for a later explicit save

### Requirement: Stable topic and independent hiding
The product topic SHALL remain stable across different records, titles and
versions. Topic mute SHALL suppress future inbox and Push publications consistently
without altering dated followup alerts or execution authority. Individual card
hide SHALL remain independent. Each record SHALL publish at most once; unmute,
restart and editing SHALL NOT replay a published or suppressed suggestion.

#### Scenario: Mute one product topic across two records
- **WHEN** the owner mutes followup_review and saves another eligible record
- **THEN** its publication is suppressed in both inbox and Push under the same topic
- **AND** the existing topic-unmute path affects only future publications
- **AND** explicitly dated followup notifications retain their separate source policy

### Requirement: Current authority and evidence withdrawal
Publication and links SHALL require the current owned artifact. Evidence changes
SHALL atomically fence pending publication/Push and redact old content and links.
Internal schedules SHALL NOT grant generic reminder control authority. Account
deletion SHALL clear sidecars. Restore SHALL quarantine pending publication and
preserve topic preferences.

#### Scenario: Edit or withdraw with a held claim
- **WHEN** a record changes or its transitive parent evidence is withdrawn while publication or Push is held
- **THEN** stale publication/dispatch fails and the old content/reference disappears
- **AND** editing a previously published record does not create another suggestion
- **AND** already dispatched outcomes remain factual while retries and passive display stay latched off

#### Scenario: Quiet hours and sealed restore
- **WHEN** a suggestion is published during quiet hours
- **THEN** inbox retains it while Push is suppressed
- **WHEN** schema56 or57 is restored from an actual sealed checkpoint
- **THEN** schema57 is available with no predecessor backfill and no pending suggestion replay
- **AND** existing topic mute remains effective after a later explicit eligible save
