## ADDED Requirements

### Requirement: One durable runtime per user
The installation SHALL allocate at most one active runtime for each account and a distinct persistent volume. Each active account SHALL own its authentication, sessions, assistant state, connector credentials, and execution progress within its runtime.

#### Scenario: Restart while another member is active
- **WHEN** a member's container is replaced or the host restarts
- **THEN** that member resumes from their own volume with one writer and other members cannot read their state

### Requirement: Shared model capacity
The installation SHALL supply read-only shared modules and model availability, and broker model calls using installation credentials while isolating user history, external actions, and provider sessions.

#### Scenario: Rotate the installation model credential
- **WHEN** two members use the same provider and its installation credential rotates
- **THEN** each retains their own conversation and neither member obtains the provider credential or the other's session

### Requirement: Independent hosted operation
The assistant SHALL support personal Google sign-in, mail lookup, and scheduled work on the deployment host without an always-on client device.

#### Scenario: Link mail from a phone
- **WHEN** a signed-in member grants Google access from a pending latest-mail request
- **THEN** the same member's runtime stores the grant and resumes the eligible read once while the member's computer is off

### Requirement: Fenced migration and recovery
The installation SHALL move each account with owner-specific evidence, forward-only handling of uncertain external effects, and a verified recovery procedure.

#### Scenario: Worker interrupted after external dispatch
- **WHEN** the runtime is recovered while a sent operation has an unknown result
- **THEN** the new owner reconciles the existing receipt and does not automatically send the operation again
