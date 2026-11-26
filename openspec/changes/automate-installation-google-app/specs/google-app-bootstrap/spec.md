## ADDED Requirements

### Requirement: A public installation prepares its own dependencies
The system SHALL support a new installer on clean Linux without existing Cloud
resources, billing, GWS configuration, private browser skills, developer files
or fixed owner-specific routing. Installation SHALL prepare member runtime,
vault recipient enrollment and browser transport from public project artifacts.

#### Scenario: A person installs the public project
- **WHEN** a new administrator installs on clean Linux with no private developer files
- **THEN** the product prepares its own Kiri and GWS runtime and starts Google connection from an empty member vault

#### Scenario: A second member connects
- **WHEN** an invited member first opens their vault and Google connection
- **THEN** their browser and recipient keys are provisioned for their own allocation without manual owner enrollment

#### Scenario: An installation is retried
- **WHEN** installation stops after allocating member storage or service configuration
- **THEN** retry reconciles the same installation and preserves its keys, accounts and completed resources

### Requirement: Billing is not an assumed setup dependency
The system SHALL inspect actual provider prerequisites and prefer a project
without billing. Billing SHALL NOT be selected implicitly, and zero billing
accounts SHALL NOT alone prevent Workspace setup.

#### Scenario: No billing account exists and billing is optional
- **WHEN** Google permits project creation without billing
- **THEN** setup proceeds without asking for or creating a billing account

#### Scenario: Google requires billing for this account
- **WHEN** the provider requires billing and no selectable account exists
- **THEN** setup identifies that specific provider prerequisite and does not report a connected account or create a paid resource

#### Scenario: Several billing accounts have the same name
- **WHEN** billing is required and the administrator chooses among offered accounts
- **THEN** selection uses the full provider identifier and no previous or sole account is silently selected

#### Scenario: A provider write has an uncertain result
- **WHEN** project creation was attempted with a recorded billing decision and the reply is lost
- **THEN** retry retains that decision and reconciles the original project before changing it or creating another project

### Requirement: Administrator connection prepares the installation app
The system SHALL continue a missing-app administrator connection in the same
isolated Kiri browser through official Google Console setup, register the web
client for the installation callback and automatically prepare member GWS.

#### Scenario: First administrator connects
- **WHEN** the administrator selects a vault Google account and no app is registered
- **THEN** setup continues after sign-in and successful registration leads to that member's service consent

#### Scenario: Setup is interrupted
- **WHEN** setup loses a response or is cancelled after creating a provider resource
- **THEN** a retry reconciles the confirmed resource before another creation and never claims service access

#### Scenario: Administrator authority expires
- **WHEN** the initiating administrator loses session or role authority
- **THEN** setup stops before further external changes or installation registration

### Requirement: Reuse the application without sharing member grants
The system SHALL share the installation application through the existing GWS
distribution while keeping each member's vault and service tokens private.

#### Scenario: Another member connects
- **WHEN** an app is registered and another member connects Google
- **THEN** the member uses that app and independently consents with their own Google account

#### Scenario: Provider restricts the audience
- **WHEN** Google requires testing membership or verification
- **THEN** the connection identifies the unfinished provider step without substituting another member's grant

### Requirement: Installation vault access remains member scoped
The installation SHALL support vault enrollment for authenticated members without
restarting or assigning a new adapter per member. Server-owned token mappings
SHALL bind account, tenant, volume, generation and registry authorization epoch.
The authority protocol SHALL NOT accept a member selector from the caller.

#### Scenario: A member joins after gateway startup
- **WHEN** an active member with the installation vault origin signs in
- **THEN** their handoff and vault session resolve only to their own private runtime

#### Scenario: A handoff or grant collides
- **WHEN** another member returns a token already registered for another scope or kind
- **THEN** registration fails and the existing mapping is preserved

#### Scenario: An event acknowledgement fails
- **WHEN** a member runtime fails to acknowledge a delivered vault lifecycle event
- **THEN** another member can receive their event and the failed durable event remains retryable

#### Scenario: Vault enrollment does not enroll a browser
- **WHEN** installation-wide vault access is enabled but only an owner-specific Google helper exists
- **THEN** the helper does not receive another member's request
