## ADDED Requirements

### Requirement: Single private installation writer
The service MUST keep its SQLite database in a private local directory, acquire a lifetime writer lock and refuse an unsafe path or a second writer.

#### Scenario: Second process opens the same installation
- **WHEN** another live writer holds the data directory
- **THEN** startup fails without opening a competing writer or modifying the database

### Requirement: Restricted temporary credentials
A temporary password MUST expire after 24 hours and grant only password-change or logout access. Password replacement MUST invalidate prior sessions atomically.

#### Scenario: Temporary administrator attempts to list users
- **WHEN** an administrator has authenticated using a temporary password and has not changed it
- **THEN** the user list remains forbidden until a successful password change issues a full session

### Requirement: Current tenant authority
Every personal operation MUST validate its current session and tenant, and administrative role MUST NOT provide access to another user's conversation.

#### Scenario: Administrator deletes a member conversation
- **WHEN** an administrator supplies an ID owned by another account
- **THEN** deletion returns the same not-found outcome as an absent ID and the member's conversation remains intact

### Requirement: Browser session boundaries
Unsafe browser requests MUST have the configured exact Origin, and the UI MUST discard late results from an earlier account generation. Private requests MUST assert the expected account and the server MUST compare it with authenticated authority.

#### Scenario: Previous account result arrives after a switch
- **WHEN** account A's response arrives after account B has authenticated
- **THEN** it neither replaces B's UI state nor reveals A's account data

#### Scenario: Another tab changes the shared cookie
- **WHEN** a tab still showing account A submits a request after another tab signs into account B
- **THEN** the server rejects the mismatched expected account without mutating B's data or cookie, and the client revalidates its workspace

#### Scenario: Delayed logout arrives after another login
- **WHEN** account A's completed logout response arrives after another tab has authenticated as account B
- **THEN** the old session is revoked, the response does not overwrite the browser cookie, and B remains authorized for its own workspace

### Requirement: Honest connected capability
The UI MUST distinguish a real account or conversation operation from unavailable model execution, and local invitation creation from email delivery.

#### Scenario: No model adapter has been implemented
- **WHEN** a user opens the conversation screen
- **THEN** model unavailability is explicit and no fabricated response or successful message send is shown
