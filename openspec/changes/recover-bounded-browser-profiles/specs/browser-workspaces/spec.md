## ADDED Requirements

### Requirement: Explicit bounded profile recovery
The system SHALL permit a human to recover an opted-in workspace profile only
within its original absolute and idle deadlines, originating authenticated app
session, account epoch, thread, account and admitted deployment. The recovered
worker SHALL use fresh handles and a blank page under human control. The system
SHALL NOT replay navigation, POST, forms, input, sends or chat turns.

#### Scenario: Process restart after an uncertain human send
- **GIVEN** a persistent-cookie login and an ambiguous human operation
- **WHEN** the application restarts and the human selects recovery
- **THEN** it grants fresh human control on about:blank and retains uncertainty
- **AND** no prior location or input is dispatched until an explicit new action

#### Scenario: Closure, expiry, revocation or deletion
- **WHEN** the workspace closes or either deadline expires, or session/account/thread authority is withdrawn
- **THEN** its profile is ineligible for recovery and exact owned storage is cleaned
- **AND** daemon failure leaves the resource discoverable without reviving authority

#### Scenario: Existing ephemeral deployment
- **GIVEN** recovery is absent or false in the operator configuration
- **WHEN** a workspace or one-shot browser starts
- **THEN** its profile remains ephemeral and its historical configuration pin is preserved

#### Scenario: A website requires a new session login
- **WHEN** its browser session cookie expires or recently changed state was not durably committed before a crash
- **THEN** recovery does not override website expiry or claim a complete login restoration
