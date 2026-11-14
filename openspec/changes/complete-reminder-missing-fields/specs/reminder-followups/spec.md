## ADDED Requirements

### Requirement: Bounded fixed-reminder missing-field completion
The authenticated current user SHALL be able to complete a missing time or literal content field of an explicit fixed-reminder creation in the immediately next supported reply, without a model/provider call or another approval screen.

#### Scenario: Missing time
- **WHEN** the owner asks to be reminded of literal text without a time and next supplies a supported bare time
- **THEN** one fixed reminder is created with the original text and timezone and the time answer's reference timestamp
- **AND** exact retry after reopen returns the same schedule receipt without a second schedule or clock shift.

#### Scenario: Missing content or civil-time component
- **WHEN** a creation already supplied time/date and a supported next reply fills literal content or only missing AM-PM/day
- **THEN** previously supplied fields remain fixed and no ambiguous/past instant is invented.

### Requirement: Draft lifetime and action isolation
A pending reminder creation SHALL remain bounded to the same current account/session/epoch/thread,15minute TTL and two answers and SHALL NOT grant general task/write authority.

#### Scenario: Unrelated, cancelled or new independent request
- **WHEN** the next reply cancels, is unsupported unrelated/question/meta prose or independently requests another action
- **THEN** it does not silently execute the old creation and a later answer cannot revive the consumed draft.

#### Scenario: Cancellation with an existing saved reminder
- **WHEN** an eligible creation draft receives an exact cancellation reply, including a final period or exclamation mark
- **THEN** only the creation draft is closed and existing saved reminders remain unchanged.

#### Scenario: Exhausted incomplete answer
- **WHEN** the second supported answer still leaves a required creation field missing
- **THEN** the draft closes and requests a complete new creation instead of asking a third field question.

#### Scenario: Changed authority or media
- **WHEN** another thread/session/owner, stale epoch, expired draft, explicit unrelated target or current/inherited media supplies an answer
- **THEN** no reminder is created through that prior draft.

#### Scenario: Restore, deletion or failed commit
- **WHEN** state is restored, its source conversation is deleted, or question/final-receipt publication fails
- **THEN** restore retires pending authority, deletion removes its sidecar, and failed mutation rolls back schedule/chat/draft consumption atomically.
