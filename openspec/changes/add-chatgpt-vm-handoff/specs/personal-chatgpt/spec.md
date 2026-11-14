## ADDED Requirements

### Requirement: Protected fresh owner session handoff
The native helper SHALL use a password-authenticated, persisted transfer-only session with an immutable ten-minute deadline, including receipt recovery. It SHALL bind the exact loopback listener, commit a ticket-separated nonce hash before browser launch, persist the issued client before exchange and seal only freshly verified credentials. The target SHALL independently verify fresh official signed identity evidence using a verifier separate from trusted local sealer reopening.

#### Scenario: Fresh protected transfer
- **WHEN** the owner signs in locally and sends the encrypted envelope through an existing trusted SSH host alias
- **THEN** the native receiver uses ordinary owner API authentication, and the target atomically commits a sealed session and authenticated receipt using its existing runtime/host
- **AND** the target independently checks the signed issuer, client, audience, subject, nonce and time evidence
- **AND** catalog/text verification and selection remain explicit existing P1 actions.

#### Scenario: Wrong owner or stale authority
- **WHEN** another owner/admin user guesses a ticket, or session/epoch/runtime/revision authority changes
- **THEN** import fails before provider access and late verification cannot install a session.

### Requirement: Durable one-owner transfer and receipt reconciliation
The target SHALL preserve existing sessions during staged verification, serialize imports, and keep durable same-digest receipts after removing the transfer key. The helper SHALL retain its encrypted pending journal until an independently fetched HTTPS receipt matches context, digest and HMAC. It SHALL contain no model/refresh/revoke path.

#### Scenario: Durable pending state before transfer
- **WHEN** the helper is ready to publish an envelope or send SSH bytes
- **THEN** it first durably publishes a refresh-disabled pending marker and retains the encrypted journal until receipt verification succeeds.

#### Scenario: Lost acknowledgement
- **WHEN** target commit succeeds but SSH or HTTP acknowledgement is lost
- **THEN** rerunning the same helper reconciles the same durable receipt without refreshing locally; forged SSH stdout alone cannot clear pending state.

#### Scenario: Conflicting import or verification interruption
- **WHEN** two imports race or verification is interrupted
- **THEN** only one exact digest can commit; identical verification may resume after its bounded lease while P1 dispatched rotating refresh remains unreclaimable.

### Requirement: Restricted authority and private native state
The Store SHALL deny transfer-only sessions operational or persistent authority creation, while typed handoff/session/status/logout operations retain normal owner/epoch/session checks. Native secret reads/writes SHALL reject unsafe ACLs on pinned descriptors and create directories private from the start.

#### Scenario: Transfer session attempts an unrelated operation
- **WHEN** a transfer-only session attempts an operational action or persistent authority creation
- **THEN** the Store rejects it while the typed transfer operations remain subject to current owner, epoch and session checks.

#### Scenario: Unsafe inherited filesystem access
- **WHEN** a native secret read or write encounters an unsafe ACL on the pinned file or ancestor descriptors
- **THEN** the helper rejects that state instead of trusting mode bits alone.

### Requirement: Monotonic handoff identity and restore invalidation
P1 and P2 SHALL reserve monotonically above retained stages and inherit subject pins only from authenticated sealed history. Restore SHALL clear all tickets/keys/receipts and regenerate runtime/host identity.

#### Scenario: Returning connection after a retained transfer
- **WHEN** a P1 or P2 connection reserves a new revision after a retained transfer stage
- **THEN** its revision exceeds all retained stages and any inherited subject pin comes from authenticated sealed history.

#### Scenario: Restored server state
- **WHEN** a server is restored
- **THEN** old transfer tickets, keys and receipts are cleared and the runtime and host identities are regenerated.

### Requirement: Bounded native user experience
The settings guide SHALL distinguish this-computer sign-in, personal HTTPS-server transfer, returning registration and add-account. It SHALL expose only safe progress and real matching-binary build instructions. No browser credential storage/download, fake installer or personal capability expansion SHALL be added.

#### Scenario: Owner chooses a connection location
- **WHEN** the owner opens personal ChatGPT connection settings
- **THEN** the guide distinguishes this-computer sign-in from personal HTTPS-server transfer and returning registration from add-account
- **AND** it shows safe progress and real matching-binary build instructions without browser credential storage/download, a fake installer or expansion of personal capabilities.
