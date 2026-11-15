## ADDED Requirements

### Requirement: Explicit memory mutations retain their domain
The assistant SHALL route supported explicit memory mutation phrases to the existing
isolated memory classifier before generic weekday or clock scheduling cues. This route
SHALL NOT itself authorize a memory mutation or replace Store target checks.

#### Scenario: Ambiguous memory correction mentions a weekday
- **WHEN** the owner explicitly corrects a memory with a weekday in its replacement fact and the stored target is ambiguous
- **THEN** the original run returns the memory clarification, preserves both facts and creates no schedule or Calendar action
- **AND** it does not dispatch an unnecessary schedule classification

#### Scenario: Quoted memory wording is a scheduled search topic
- **WHEN** an owner asks for a supported recurring search containing a quoted or non-command mention of a memory phrase
- **THEN** normal mission classification can create the requested schedule instead of suppressing it

### Requirement: Source presentation is not arithmetic
Report completion SHALL NOT require a calculation receipt solely because the owner
asks in Korean to separate source claims for display.

#### Scenario: A requested report separates conflicting records
- **WHEN** the owner requests a private report and asks to show source claims separately without asking for arithmetic
- **THEN** actual source-read receipts can produce the saved review-required report without an invented calculation

#### Scenario: Division is explicitly requested
- **WHEN** the requested report divides an amount by an explicit supported quantity or numeric denominator
- **THEN** source reads alone cannot satisfy its calculation prerequisite

### Requirement: Local draft preparation preserves an explicit send prohibition
A supported explicit request to prepare a local mail draft SHALL remain admissible
when the same owner request prohibits sending. No-send wording SHALL NOT grant sending
or erase a separate prohibition on preparing the draft.

#### Scenario: Missing recipient with no-send instruction
- **WHEN** the owner delegates local draft preparation, supplies no recipient and explicitly prohibits sending
- **THEN** the original task can wait for its recipient and resume into one pending-approval draft
- **AND** no send dispatch is created, including after reply retry

### Requirement: Tool descriptions fit the supported wire contract
Built-in tool descriptions SHALL remain within the existing protocol limit and
state exact evidence requirements without exceeding that limit.

#### Scenario: Evidence tools coexist with other supported read tools
- **WHEN** a shared or personal model request includes the evidence calculation tool
- **THEN** the actual combined tool request can encode and continue within existing limits
