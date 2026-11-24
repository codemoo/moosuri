## ADDED Requirements

### Requirement: A conversation owns independently executable research tasks

The member service SHALL retain one original chat request and independent, authorized work items. Each item SHALL record its status and be executable after the page closes. Duplicate admission MUST resolve to the original group.

#### Scenario: One sibling needs input

- **WHEN** a member requests a memory search with a term and a conversation search that lacks a term
- **THEN** the first task can finish while the second retains a question and the ordinary chat can continue.

#### Scenario: Lost acknowledgement

- **WHEN** the member retries the same admission after the server has stored it
- **THEN** the original turn, task identities and completed reads are preserved.

### Requirement: Guidance belongs to a particular child and question

The member service SHALL bind instructions to the member, thread, child, current revision and, for answers, the current question. Status reads SHALL have no dispatch side effects.

#### Scenario: Clarification after refresh

- **WHEN** the member answers a current saved question twice with the same request identifier
- **THEN** the task continues once and the other tasks keep their own instructions.

### Requirement: Reports and reusable procedures have verified provenance

Members SHALL see observed results and may create an identifiable report from a completed run or repeat its proven search procedure with a new input. Reads SHALL resolve sources against current access and revisions.

#### Scenario: Source changes after a report is saved

- **WHEN** a source becomes unavailable
- **THEN** its historical count remains and the report no longer reveals the old source body.

### Requirement: Browser work can pause for lack of progress

A member browser task SHALL pause for member input when repeated steps produce the same observation without progress.

#### Scenario: A fresh instruction follows a stalled read

- **WHEN** a member directs a stalled task to continue from a new observation
- **THEN** the original worklog records the pause and the authorized continuation.
