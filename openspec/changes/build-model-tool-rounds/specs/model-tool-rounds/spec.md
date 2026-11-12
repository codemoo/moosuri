## ADDED Requirements

### Requirement: Completed calls require a fully validated round
The system SHALL return calls only after valid terminal state, exact model identity,
complete reconciled call objects and clean bounded HTTP EOF. Previews SHALL NOT
authorize execution.

#### Scenario: Later HTTP error
- **WHEN** a terminal and subsequent error arrive in different HTTP chunks
- **THEN** no executable tool round is returned

#### Scenario: Held tail
- **WHEN** the server holds the connection after terminal
- **THEN** cancellation/deadlines remain active and only clean EOF returns completed calls

### Requirement: Continuations preserve private history and dialect identity
The system SHALL preserve ordered reasoning, assistant phase and call/result
correlation, bound checkpoints and pin protocol/model/effort.

#### Scenario: Sparse phase or resumed checkpoint
- **WHEN** later Responses metadata omits an observed phase or a checkpoint is loaded
- **THEN** the phase remains preserved and mismatched checkpoint pins are rejected

#### Scenario: Incomplete result mapping
- **WHEN** results omit, duplicate or invent a pending call ID
- **THEN** encoding fails before network dispatch

### Requirement: Tool support is explicit and bounded
The system SHALL default function capability off, bound calls/arguments/schemas/
checkpoints and share existing network and cancellation controls.

#### Scenario: Unverified capability
- **WHEN** function support is not enabled
- **THEN** tool requests fail before dispatch even after a successful text probe

#### Scenario: Existing text client
- **WHEN** a text-only caller receives a valid terminal
- **THEN** its completion behavior is preserved and no tool executes
