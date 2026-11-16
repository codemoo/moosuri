## ADDED Requirements

### Requirement: Public everyday continuity cases

The evaluator SHALL execute three independently gradable synthetic workflows
through actual authenticated HTTP, Runtime and SQLite: in-flight owner guidance,
owner clarification and receipt-bound reminder change/cancellation.

#### Scenario: Guidance after completed work

- **WHEN** the owner adds guidance after a saved read and calculation and replays that same instruction
- **THEN** the same run and attempt SHALL apply one receipt, retain the completed receipts once, reflect the new requested fact, and survive reopen without another dispatch

#### Scenario: Clarification after a saved read

- **WHEN** a mission asks for a missing delivery criterion and the world reopens before the owner answers
- **THEN** the same wait, original run, acceptance and completed read SHALL survive, one owner answer SHALL resume work, and free-form context SHALL retain semantic owner review

#### Scenario: Receipt-bound reminder control

- **WHEN** the owner moves and cancels the immediately preceding reminder while another reminder exists
- **THEN** the exact receipt identity SHALL advance through create/edit/revoke, exact replay SHALL return the original run, the other reminder SHALL remain unchanged, and reopen SHALL preserve cancellation without model/tool dispatch

### Requirement: Honest stage and grader evidence

The evaluator SHALL report planned stage status, per-stage model/tool budgets,
actual counts and postconditions. Setup failures SHALL remain planned/not_run and
workflow failures SHALL remain fail. Later stages SHALL remain not_run. Token
missingness SHALL remain unknown. Every selected case SHALL reject tampered
observations, and final exact prose SHALL NOT define task success.

#### Scenario: Apparatus or stage failure

- **WHEN** setup, execution, a stage budget or a postcondition fails
- **THEN** the case SHALL remain in the original denominator, the failure SHALL be visible, and unavailable observations SHALL NOT become successful evidence
