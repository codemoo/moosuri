## ADDED Requirements

### Requirement: Installation preserves private state boundaries
The app SHALL provide a standalone manifest and public-only offline shell.
The worker SHALL NOT cache private API responses, credentials, chat content or
private files, and SHALL NOT queue mutations while offline.

#### Scenario: Offline private request
- **WHEN** the network fails after the public shell has installed
- **THEN** a known app navigation can render its public shell
- **AND** a private API or media request is not satisfied from cache
- **AND** private data still requires current server authentication

### Requirement: Update does not discard a mounted draft
A new public-shell version SHALL wait for existing tabs to close rather than
forcing a reload. Connection loss SHALL be announced without replacing local
mounted composer state.

#### Scenario: Connection loss and recovery
- **WHEN** the user has an unsent draft and connectivity changes
- **THEN** the same tab retains that draft without automatically sending it
- **AND** an initially failed boot retries its server session on reconnect
