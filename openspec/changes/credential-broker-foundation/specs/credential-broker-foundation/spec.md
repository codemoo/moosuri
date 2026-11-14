## ADDED Requirements

### Requirement: Device pairing uses the real current owner
Pairing SHALL consume one current normal-session challenge once and SHALL pin
account, tenant, epoch, installation, origin, device public key and generation.

#### Scenario: Foreign proof and concurrent consumption
- **WHEN** another key signs a challenge, a helper/restricted/expired session requests pairing, or installation/epoch changes
- **THEN** no enrollment is created
- **AND** two correct concurrent consumptions admit exactly one enrollment

### Requirement: Device requests have bounded asymmetric authentication
Every admitted device request SHALL bind method, path, canonical typed body,
owner/device context, fresh single-use challenge and expiry using Ed25519.

#### Scenario: Replay and changed transport context
- **WHEN** a successful signature is replayed or method/path/body/origin/generation changes
- **THEN** no new binding, commitment or receipt is admitted
- **AND** cookie/Origin authority is rejected on device routes

### Requirement: Local grants are the automatic scope ceiling
Only an actual local selection SHALL create an immutable local grant. Server
metadata SHALL NOT create/broaden it. Compatible repeated use SHALL be automatic.

#### Scenario: Repeated selection-free compatible use
- **WHEN** a second compatible fixed probe uses an active local selection
- **THEN** local checks accept it without another selection or approval callback

#### Scenario: Forged scope and secret-bearing generic input
- **WHEN** owner, resource digest, host, adapter version, item/key/binding/grant epoch or expiry differs
- **THEN** local dispatch is denied
- **AND** generic URL/header/credential/recipient parameters have no request representation

### Requirement: Writer commitment has one original owner and identity
The single writer SHALL validate current authority and adopt exactly the stored
original typed invocation, broker generation, digest, binding and expiry atomically
against revoke. Production arbitrary effect/digest minting SHALL remain unavailable.

#### Scenario: Revocation wins
- **WHEN** an old precheck reaches the writer after revoke
- **THEN** no new commitment is created

#### Scenario: Commitment wins or acknowledgement is lost
- **WHEN** commitment precedes revoke or its reply is lost
- **THEN** the original device recovers the same identity/nonce as possibly in flight
- **AND** another broker/generation cannot adopt or resend it

### Requirement: Native journal is private, single-owner and conservative
The native engine SHALL lock private owner files, fsync nonsecret state before
adapter entry and fail closed on missing/corrupt/truncated state or stale enrollment.

#### Scenario: Restart and competing process
- **WHEN** a process exits after prepare/commitment/entry or another process opens its live journal
- **THEN** the second live owner is denied and retained restart work cannot enter an adapter
- **AND** entered work without a receipt remains unknown, never inferred not-dispatched from disconnection

#### Scenario: Positively suppressed dispatch
- **WHEN** local revoke or settlement occurs before durable adapter entry
- **THEN** the known unentered commitment may be signed as not-dispatched
- **AND** this does not claim upstream revocation or erasure

### Requirement: Device receipts settle without resurrecting output
A signed receipt SHALL bind the full original commitment/outcome/fixed projection.
Exact duplicates SHALL be idempotent; changed signatures or payloads SHALL fail.

#### Scenario: Late private settlement
- **WHEN** an authentic original-owner receipt arrives after cancel/logout/revoke/restore quarantine
- **THEN** private settlement evidence is retained once
- **AND** cancelled conversation output is never restored

#### Scenario: Malicious provider echo
- **WHEN** the test-only fixed adapter emits secret/TOTP canaries in success or error text
- **THEN** a new fixed enum projection omits the raw body
- **AND** no production adapter is implied by this synthetic evidence

### Requirement: Restore removes authorizing enrollment and preserves evidence
Schema37 SHALL migrate transactionally and restore SHALL quarantine broker grants,
enrollments and challenges while preserving non-authorizing commitments/receipts.

#### Scenario: Migration late failure and restore
- **WHEN** a late schema37 statement fails or an old enrollment is replayed after restore
- **THEN** migration rolls back/retries intact and old enrollment cannot authorize new work
- **AND** existing ciphertext contracts remain unchanged

### Requirement: Foundation availability and trust claims are bounded
Routes SHALL be absent by default. Production credential dispatch SHALL remain off
until adapter review. Documentation SHALL disclose unsigned-client/server-task and
coordinated-directory rollback limits.

#### Scenario: Paired malicious assistant and unshipped distribution
- **WHEN** the assistant invents an in-scope task or no signed client has shipped
- **THEN** pairing/signatures are not described as independent user-intent or distribution proof
- **AND** the full credential-use/v1 feature remains incomplete
