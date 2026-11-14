## ADDED Requirements

### Requirement: Owner-scoped personal text connection

The application SHALL isolate personal ChatGPT registration and credential state by local owner.

#### Scenario: Another owner guesses a binding

- **WHEN** a member or administrator-as-user requests another owner's binding
- **THEN** the operation SHALL fail before credential decryption or provider HTTP.

#### Scenario: Shared default receives a personal target

- **WHEN** an administrator attempts to set a personal ChatGPT connection as installation default
- **THEN** the application SHALL reject that selection.

### Requirement: Verified durable local login

The application SHALL validate official loopback OAuth before saving usable credentials.

#### Scenario: Issued client precedes redemption

- **WHEN** a callback produces an initial issued registration
- **THEN** the application SHALL persist that mapping before exchanging its code.

#### Scenario: Returning identity changes

- **WHEN** a returning callback supplies a different client or signed subject
- **THEN** the application SHALL reject replacement of the selected verified identity.

#### Scenario: Restart reopens credentials

- **WHEN** a verified session is reopened from private storage
- **THEN** authenticated sealing SHALL enforce its owner and runtime context without unchecked identity deserialization.

### Requirement: Serialized rotating refresh

The application SHALL commit refresh admission before sending a rotating refresh token.

#### Scenario: Concurrent renewal

- **WHEN** two tasks need renewal for the same session generation
- **THEN** at most one SHALL dispatch that generation's refresh token.

#### Scenario: Rotation outcome is lost

- **WHEN** a process loses a potentially successful dispatched refresh
- **THEN** restart SHALL require reauthentication instead of replaying its old refresh token.

#### Scenario: Identity validation fails after rotation

- **WHEN** a refresh returns HTTP200 but subsequent JWKS fetching or replacement validation fails
- **THEN** the application SHALL quarantine that session without replaying the previous rotating token.

#### Scenario: Refresh endpoint rejects temporarily

- **WHEN** the refresh-token endpoint itself returns a known temporary rejection
- **THEN** the application MAY preserve the old session for a bounded later retry.

### Requirement: Pinned personal text execution

The application SHALL require exact account-specific text proof before personal selection.

#### Scenario: Personal target becomes unavailable

- **WHEN** the saved target is disconnected or its pinned revision becomes stale
- **THEN** the preference SHALL remain unavailable without shared-provider fallback.

#### Scenario: Capability is not implemented

- **WHEN** P1 personal execution requests tools, attachments or background work
- **THEN** the application SHALL leave that capability unavailable while preserving existing shared flows.

#### Scenario: Earlier turns contain attachment context

- **WHEN** a personal text turn follows ready document or image dependencies in its conversation
- **THEN** admission SHALL reject it before private body loading or provider IO while keeping the explicit selection.

### Requirement: Revocation and restore fence authority

The application SHALL prevent revoked session or connection authority from installing late results.

#### Scenario: Remote disconnect fails

- **WHEN** local disconnect cannot confirm provider revocation
- **THEN** local usable tokens SHALL be cleared and remote status SHALL remain unconfirmed.

#### Scenario: Sealed restore creates a new root

- **WHEN** a checkpoint is restored into a new installation
- **THEN** personal sessions and proofs SHALL be quarantined and a new host identity SHALL be created.
