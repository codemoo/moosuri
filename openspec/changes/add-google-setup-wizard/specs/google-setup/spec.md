## ADDED Requirements

### Requirement: The installer can prepare one independent Google app
The system SHALL provide an administrator-only wizard showing the callback derived
from its configured origin, Google setup instructions, and bounded web-client JSON
validation/import. Members SHALL use their own personal Google consent, without
creating another Cloud project for the same installation.

#### Scenario: Validate a downloaded client file
- **WHEN** an administrator imports a Google web-client file
- **THEN** the server checks its structure and exact configured callback without contacting Google
- **AND** the UI distinguishes local validation from successful personal authorization
- **AND** imported endpoints and headers cannot change credential destinations

### Requirement: Installation configuration preserves credential boundaries
The system SHALL encrypt the app secret under the server-managed installation key,
validate current admin authority and expected revision at save, and never expose the
secret in responses, logs, browser persistence or model context. Operator-file
configuration SHALL be explicit and prevent web writes for that process.

#### Scenario: Concurrent save or an operator override
- **WHEN** a stale administrator saves a file or operator-file configuration is active
- **THEN** the write is refused without replacing the effective setup
- **AND** a restarted process without the file uses only an eligible persisted setup

### Requirement: Setup changes cannot misbind personal grants
The system SHALL bind OAuth exchanges and mail credential use to the selected app
identity and revision, preserve pending authorization, and fence stale refresh success,
refresh failure and source-run completion after configuration changes.

#### Scenario: A secret rotates during a refresh
- **WHEN** a refresh started with an older app revision finishes after a permitted rotation
- **THEN** it cannot overwrite current credentials or revoke a valid personal connection
- **AND** an old source lease cannot label the newer setup as needing reconnection

### Requirement: Connection tests are personal and do not send mail
The wizard SHALL test only the requesting administrator's own Google authorization
and Gmail profile. Setup return routes SHALL be selected by stored flow state, not
an arbitrary callback parameter.

#### Scenario: A member or another account's connection is supplied
- **WHEN** the caller lacks current admin authority or supplies another user's connection
- **THEN** no provider request or personal data disclosure occurs
- **AND** no connection check sends mail or enables source collection
