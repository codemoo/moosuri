## ADDED Requirements

### Requirement: Unsent instruction drafts follow their own continued carrier
The assistant UI SHALL preserve an unsent instruction draft when the same owned
run and thread advertises a newer supported attempt. It SHALL use that attempt
only for an explicitly submitted fresh request and SHALL NOT automatically send
the draft or move it to another carrier or an older attempt.

#### Scenario: The same task resumes while an instruction is being written
- **GIVEN** an unsent draft for a supported task carrier
- **WHEN** that same run and thread resumes at a higher supported attempt
- **THEN** the draft remains editable and sendable, with no automatic POST
- **AND** an explicit send uses a new request UUID and the current attempt
- **AND** a subsequent stale view cannot rewind the saved draft target

#### Scenario: An uncertain earlier request is still being reconciled
- **GIVEN** an unresolved instruction POST and a separately typed later draft
- **WHEN** the task advances to a higher attempt
- **THEN** receipt reads and explicit replay keep the original submitted UUID, text and attempt
- **AND** after that request settles, the later unsent draft can be explicitly sent as a new request to the same current carrier

#### Scenario: Another task becomes eligible
- **GIVEN** a draft already bound to one carrier
- **WHEN** a different run or thread becomes the live target
- **THEN** the UI preserves the draft and does not silently submit it to that target
