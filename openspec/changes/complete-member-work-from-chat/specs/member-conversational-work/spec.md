## ADDED Requirements

### Requirement: Everyday language can lead to an actual member result

The member service SHALL route a reworded request to the same permitted outcome and SHALL distinguish a question, a new task, an instruction to an existing task and an unavailable capability. The member SHALL receive evidence of the requested outcome in the originating conversation.

#### Scenario: Research and a private document in one message

- **WHEN** a member asks to look up a past decision and packing items and save them on one page
- **THEN** the resulting work records the sources it actually read and links one private document containing the requested parts.

#### Scenario: The member chooses ordinary chat or starts entering the next request

- **WHEN** the member manually resends a pending ordinary message after a reload
- **THEN** the selected route and request ID survive, and accepting the original submission preserves an independently edited draft.

#### Scenario: One follow-up refers to the second task

- **WHEN** two tasks are visible and the member narrows only the second one
- **THEN** that task alone receives the new condition and a later question can refer to its current authorized evidence.

#### Scenario: Continue a plan after a correction

- **WHEN** a member corrects the day of a meeting or changes the headcount for a fare calculation and asks for the updated result
- **THEN** ordinary chat uses the earlier time or fare and answers in the same conversation without creating a recurring watch. An explicit request for notification when a saved item changes can start a watch.

### Requirement: Member input and external effects keep their identity

The runtime SHALL preserve the member, task, current source versions, input question and approved external action across reconnects. A provider receipt SHALL distinguish a completed external action from a draft, a handoff and an unknown outcome.

#### Scenario: Change during final review

- **WHEN** the member changes the recipient or content of a prepared mail before dispatch
- **THEN** only the newly reviewed version can be sent and recovery of an unknown send does not send a second message.

#### Scenario: Browser handoff during an application or order

- **WHEN** the member takes over the task browser and returns control after entering private information
- **THEN** the assistant resumes the same task from a fresh observation, preserves completed steps, and reports submission only when confirmed.

### Requirement: Recurring work and reuse stay within the original scope

The runtime SHALL retain the timing, scope and cessation of a member's recurring check. Reusing a workflow SHALL resolve the new input and current permissions. A revoked source or connection SHALL cease to supply private data to later output.

#### Scenario: A member retries a stop after its acknowledgement was lost

- **WHEN** a member stops a watch, loses the acknowledgement and creates another watch with the same name
- **THEN** a manual retry submits the original watch ID, revision and request ID. A definitive conflict returns the member to the current list.

#### Scenario: An explicit cadence and lifetime in chat

- **WHEN** a member specifies a cadence and lifetime for a local or public watch
- **THEN** the service records the supported values or leaves the draft editable for correction. Quoted search terms do not supply timing.

#### Scenario: A member combines hours and minutes

- **WHEN** the requested check interval is one hour and thirty minutes
- **THEN** the member can correct the unsupported 90-minute interval before a watch is created. A supported interval with a lifetime of 48 hours is stored as two days.

#### Scenario: Repeated identical observation

- **WHEN** a member requests change-only alerts and two runs observe the same change
- **THEN** one notification with a source is visible and the original worklog identifies the relevant occurrence.

#### Scenario: A once-approved operation is suggested again

- **WHEN** the assistant applies a previous workflow to a new input
- **THEN** it verifies the new sources and the current authorization before any external operation.
