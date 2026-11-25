## ADDED Requirements

### Requirement: Personal answers respect the member's current preferences

A Kiri member SHALL be able to state a lasting personal preference in ordinary conversation and have it applied to a relevant later answer within or across conversations. The answer SHALL give priority to the current request and use only eligible memory belonging to that member.

#### Scenario: A member states their own usual preference

- **WHEN** a member says that they find aisle seats more comfortable on trains
- **THEN** their preference appears in their existing editable memories.

#### Scenario: Relevance survives a change of thread or a long conversation

- **WHEN** the member asks for a seat recommendation in a new conversation or beyond the recent turn window
- **THEN** the ordinary answer can use the current preference without a reminder or a research task.

#### Scenario: This journey is an exception

- **WHEN** the member asks for a window seat this time
- **THEN** the reply follows the request and the next independent journey retains the usual preference.

#### Scenario: Correction and withdrawal

- **WHEN** a retrieved preference is corrected or forgotten
- **THEN** superseded evidence cannot supply later answers, including a pending reply or reused conversation history.

#### Scenario: Scope and abstention

- **WHEN** the query is unrelated, belongs to another member, or has no matching current evidence
- **THEN** the answer receives no such preference. A hypothetical, reported, quoted or temporary choice does not create a lasting preference.
