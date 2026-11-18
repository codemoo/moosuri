## ADDED Requirements

### Requirement: Model-independent latest inbox request

A direct, unambiguous current-owner request for the latest received Gmail message SHALL use the current account's configured Gmail read connection and report its result or connection status before model dispatch. Quoted, negated, unrelated, and compound requests SHALL follow ordinary routing.

#### Scenario: Text-only model and no Gmail connection
- **WHEN** the owner asks which email arrived most recently and Gmail is unavailable
- **THEN** the assistant identifies the connection requirement and provides the Gmail settings entry point

#### Scenario: More than one readable account
- **WHEN** the owner does not identify one mailbox and more than one eligible connection is active
- **THEN** the assistant reports the ambiguity without reading any mailbox

### Requirement: Bounded inbox metadata

The route SHALL compare actual Gmail internal timestamps across its checked records and describe the coverage. It SHALL use metadata only and keep mailbox content out of the model transcript. Session and connection authority SHALL remain valid when the result is saved.

#### Scenario: Provider listing order differs from receive order
- **WHEN** listed messages are returned out of order
- **THEN** the assistant selects the most recent checked timestamp and reports any incomplete search

#### Scenario: Permission is withdrawn during a read
- **WHEN** the session or read connection is revoked before completion
- **THEN** the pending read cannot expose the result in chat
