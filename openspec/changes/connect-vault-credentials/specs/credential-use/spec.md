## Purpose

Allow assistant tasks to use previously connected vault credentials through scoped trusted execution while preserving user isolation, truthful waiting states and non-disclosure to model context.

## ADDED Requirements

### Requirement: Automatic scoped reuse
The system SHALL reuse a valid user-service credential binding for an authorized compatible task without repeated routine confirmation.

#### Scenario: Second authorized request
- **WHEN** the user requests the same supported operation with an active matching binding
- **THEN** the broker uses that binding after internal scope checks without another account picker

### Requirement: Secret confinement
Secrets MUST remain inside the trusted credential execution path and MUST NOT enter model context, general tool output, logs or community exports.

#### Scenario: Credential-bearing provider error
- **WHEN** a provider returns an error containing authentication material
- **THEN** the adapter emits a bounded fixed error/projection without that material

### Requirement: Unavailable credential recovery
A locked, revoked or offline broker SHALL preserve the original task in a waiting state without server escrow, false completion or duplicate external effects.

#### Scenario: Lost acknowledgement after remote success
- **WHEN** the broker restarts after sending an effect but before its acknowledgement reaches the server
- **THEN** recovery reconciles the same effect or preserves unknown outcome instead of issuing a new send

### Requirement: Scope and lifecycle enforcement
The trusted broker MUST bind operations to the user's allowed service, exact destination, resource and recipient scope, and current credential/grant versions; receipts MUST be device-authenticated when used as server-independent evidence.

#### Scenario: Changed recipient or revoked binding
- **WHEN** a package changes the recipient outside the original task or replays a revoked grant
- **THEN** the broker rejects dispatch without exposing secrets or treating local revocation as confirmed upstream revocation

### Requirement: Honest execution modes
Strong vault mode SHALL keep plaintext on an independently trusted device; optional server-held connector tokens MUST be explicitly distinguished from that mode.

#### Scenario: Locked device
- **WHEN** the trusted device is locked or offline
- **THEN** the original run waits, releases scarce execution slots and resumes only within still-valid authorization, without copying vault secrets to the server

### Requirement: Single broker effect ownership
Each adopted effect MUST be pinned to one authenticated broker identity and generation. The system MUST NOT automatically transfer an adopted effect to another broker or treat disconnection as proof that dispatch did not occur.

#### Scenario: Two brokers after lost acknowledgement
- **WHEN** broker A may have dispatched an effect and broker B becomes available while A disconnects or replies late
- **THEN** the same effect is reconciled through A's durable journal or remains unknown; B does not resend it, and A's authenticated late receipt is retained

### Requirement: Device-owned grant ceilings
The broker MUST enforce grants stored on the trusted device and MUST NOT accept a new or broader grant merely because it arrived through the paired server channel. v1 MUST describe in-scope request construction as delegated server authority, not proof of genuine user intent under a malicious server.

#### Scenario: Fabricated grant from the paired server
- **WHEN** a correctly authenticated server submits a new task with an invented or broader grant
- **THEN** the broker rejects expansion beyond its local grant; reports do not claim that pairing alone can detect forged tasks inside an existing delegated scope

### Requirement: Revocation and dispatch commitment
New authenticated dispatches MUST obtain an online commitment serialized against the current account and binding epochs. Revocation before commitment MUST prevent it; previously committed work MUST remain in flight or unknown until reconciled.

#### Scenario: Held request crosses revocation
- **WHEN** a prechecked uncommitted request loses connectivity and the account is revoked before the broker reconnects
- **THEN** the broker cannot dispatch using the old check or permit and must validate current authority

#### Scenario: Revocation follows commitment
- **WHEN** a dispatch commitment precedes revocation and the broker acknowledgement is unavailable
- **THEN** the operation remains in flight or unknown and local revocation remains pending until acknowledgement or fencing is established
