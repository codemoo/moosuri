## ADDED Requirements

### Requirement: Explicit saved followup date notifications
The system SHALL schedule each distinct future due/review date of an explicitly
saved active unmuted followup once through the existing fixed reminder loop.
Future snooze SHALL create one notice at snooze and coalesce earlier dates into
it, retaining later dates. Past dates without future snooze and migrated history
SHALL NOT backfill. Dates after the existing2100 horizon SHALL remain saved and
briefing-only. Notification time SHALL invoke no model or external send tool.

#### Scenario: Future dates, old dates and snooze
- **WHEN** the owner saves due/review dates, including equal dates or an explicit future snooze
- **THEN** receipts identify actual distinct scheduled instants and snooze collapses earlier dates
- **AND** old past dates alone produce no new occurrence

#### Scenario: Offline restart after multiple dates
- **WHEN** the worker starts after several dates of one revision elapsed
- **THEN** only the latest elapsed date publishes and earlier pending dates coalesce
- **AND** reopen, lost save ACK and clock reversal do not replay a delivered same instant

### Requirement: Artifact revision fences and truthful receipts
Notices SHALL show the saved artifact revision/date and link only current owned
records. Edits, date/snooze/lifecycle/mute changes, deletion and transitive evidence
withdrawal SHALL atomically retire old pending reminder/Push authority and
redact/dismiss old snapshots. Dispatched results SHALL remain factual while old
passive display/retry authority stays suppressed. Internal schedules SHALL grant
no ordinary reminder control authority.

#### Scenario: Mutation races publication or Push
- **WHEN** an owner edits/resolves/cancels/mutes/snoozes/deletes a followup or its evidence is withdrawn while a reminder/Push claim is held
- **THEN** that stale claim cannot publish or dispatch, unscanned/queued notices cannot Push, and old artifact links disappear
- **AND** an already dispatched outcome remains recorded while passive display is rejected

#### Scenario: Current record navigation and quiet hours
- **WHEN** a followup notice is listed by its owner
- **THEN** it shows the actual pinned revision/date and opens the current owned record
- **AND** quiet hours retain the inbox notice while suppressing Push

### Requirement: Existing owner, source and restore boundaries
Followup notifications SHALL retain current account/epoch checks and a stable
source preference across future dates/revisions. Unmute SHALL not replay old
notices. Sealed restore SHALL quarantine future execution and Push; explicit
resave SHALL arm only future dates. Another account SHALL not view/control the
record, source, notification or internal schedule.

#### Scenario: Stable source mute and another owner
- **WHEN** a source is muted and its followup is edited to a new future date
- **THEN** the new notice remains withheld and unmute does not replay it
- **AND** another owner cannot access or change either source or artifact

#### Scenario: Sealed restore keeps history inert
- **WHEN** a snapshot with followup reminders is restored
- **THEN** history is preserved while schedules/Push remain quarantined
- **AND** owner resave creates authority only for remaining future dates
