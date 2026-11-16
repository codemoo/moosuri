## ADDED Requirements

### Requirement: Invalid source choice preserves the original wait

The evaluator SHALL run one real HTTP/Runtime/SQLite source-choice lifecycle using
two synthetic household estimates. Public inputs SHALL remain separate from the
independent grader.

#### Scenario: Ambiguous noncandidate answer

- **WHEN** an owner gives a noncandidate answer to an actual source-choice wait
- **THEN** the documented invalid-input response SHALL leave the wait, notice, original work, authority/deadline and reply rows unchanged with no inference

#### Scenario: Corrected selection and restart

- **WHEN** the owner chooses one actual current candidate and replays that answer
- **THEN** exactly one reply SHALL retain the original run/request/acceptance/model/deadline
- **AND** restart SHALL resume that choice using the original completed source-read receipts without rereading

#### Scenario: Cancel the resumed final response

- **WHEN** the owner cancels while the resumed final response is held and replays cancellation
- **THEN** one control SHALL disable the original grant
- **AND** exactly one actual supervised worker SHALL be observed before release, then reaped and locally settled to zero while the same Runtime remains alive and its observer remains open
- **AND** cancellation without final candidate, output, artifact/external effect or visible retired notice SHALL be observed before shutdown
- **AND** subsequent reopen SHALL retain cancelled state and truthful uncertainty without new dispatch

### Requirement: Honest handling evidence

The case SHALL retain planned/handling eligibility on failure and SHALL NOT count
cancellation as achieved task completion. Shared stage budgets SHALL count actual
dispatches/tools; unknown usage SHALL remain unknown. Provider terminal ledger
entries or Runtime shutdown SHALL NOT substitute for live worker retirement.

#### Scenario: Tampered cancellation observations

- **WHEN** the actual successful handling observation is checked against required negative controls
- **THEN** accepted ambiguity, replaced waits, invented candidate receipts, repeated reads, duplicate replies, late results, missing live worker settlement and visible retired notices SHALL each be rejected
- **AND** these controls SHALL remain separate from task-completion samples

#### Scenario: Setup failure and interrupted usage

- **WHEN** setup fails or cancellation leaves model usage incomplete
- **THEN** planned and handling denominators SHALL retain the case
- **AND** missing token usage SHALL remain unknown rather than zero
