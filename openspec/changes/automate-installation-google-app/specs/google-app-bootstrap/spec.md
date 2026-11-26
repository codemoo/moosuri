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

### Requirement: The public browser launcher pins one local engine
The installation SHALL select its supported local engine in trusted deployment
configuration. Launch, inspection, profile lifecycle and cleanup SHALL use the
same engine. Engine support SHALL require actual Linux containment and Rust
launcher acceptance, preserving the browser sandbox and private-network denial.

#### Scenario: A legacy Docker configuration is loaded
- **WHEN** the deployment JSON omits engine selection
- **THEN** the Docker endpoint and prior deployment fingerprint remain unchanged

#### Scenario: A Kiri browser deployment starts
- **WHEN** the selected engine is Kiri
- **THEN** the launcher requires the supported actual Kiri component version and uses its fixed local socket for all resource operations

#### Scenario: A member tries to supply an engine endpoint
- **WHEN** an arbitrary socket path or remote URL is supplied as engine selection
- **THEN** configuration is rejected before launching a worker

#### Scenario: One of two members closes their worker
- **WHEN** the first member's worker shuts down in an accepted Linux deployment
- **THEN** the second member's page stays available and the cleaned slot can be reused

### Requirement: Member allocation survives a compatible Kiri release
The member provisioner SHALL accept only explicitly verified Linux Kiri API
versions. A compatible release SHALL preserve the existing allocation identity,
UID, volume and inspected isolation contract. Browser and member engine support
SHALL be stated separately.

#### Scenario: A compatible member engine is used
- **WHEN** the member provisioner runs on verified Kiri 0.2.1 or 0.2.2 with Linux API 1.47
- **THEN** it reconciles the original allocation profile without replacing identities or weakening inspected limits

#### Scenario: An unverified engine is presented
- **WHEN** the Kiri version, operating system or API version is outside the verified set
- **THEN** member provisioning stops before creating or starting a container

#### Scenario: One member requires recovery
- **WHEN** the lifecycle service recovers a stopped member in an accepted deployment
- **THEN** the member keeps its allocation and session data while the running sibling retains its process and access


### Requirement: A fresh capsule owns each authentication secret
The installation SHALL use a fresh official Chrome capsule and a one-use guest
recipient for every Google connection. Ciphertext SHALL authenticate the
installation, account, tenant, volume, generation, authorization epoch,
challenge, member and selected Google email, and callback origin. The private
key and decrypted password SHALL stay inside that capsule.

#### Scenario: A vault prepares delivery
- **WHEN** the gateway reserves a recipient
- **THEN** Chrome remains stopped until successful decryption and a separate reauthorized release

#### Scenario: A different allocation receives the packet
- **WHEN** another allocation, challenge or recipient key attempts decryption
- **THEN** the packet is rejected before Chrome starts

#### Scenario: A Google password page identifies another account
- **WHEN** the visible password recipient differs from the selected vault account or Google rejects the browser
- **THEN** no password is submitted and the attempt ends without fallback

#### Scenario: A connection is cancelled
- **WHEN** the controller closes or a termination signal arrives
- **THEN** the owned browser and temporary profile are removed, another member remains unaffected, and a success receipt is not emitted

#### Scenario: The provider returns to the callback
- **WHEN** the page contains a denial or a mismatched callback state
- **THEN** the worker does not report return success; only the gateway's actual callback processing can grant a connection
