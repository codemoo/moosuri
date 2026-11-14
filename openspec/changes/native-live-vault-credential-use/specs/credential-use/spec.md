## ADDED Requirements

### Requirement: Native live credential isolation and freshness
The native broker SHALL unlock the live independent vault using actual vault-core
only on the trusted device. The master password and credential SHALL NOT enter the
assistant, model, general tool output, argv or journal. The broker SHALL verify
current owner/header/inventory and selected item again after decrypt before entry.

#### Scenario: Connected compatible reuse
- **WHEN** another valid request targets the same selected GitHub repository
- **THEN** the current live credential is reused without another selection/approval
- **AND** only the signed allowlisted result reaches the assistant

#### Scenario: Transient offline suspension
- **WHEN** the vault temporarily cannot be reached before adapter entry
- **THEN** dispatch is suspended and valid native in-memory unlock is retained
- **AND** reconnection automatically revalidates current state before continuing
- **AND** restart cannot redrive retained work using that in-memory permission

#### Scenario: Peer item or key change
- **WHEN** a selected item is changed/deleted or the current header/key changes
- **THEN** the old native root/session is retired without provider entry
- **AND** fresh native reconnection/unlock is required
- **AND** a change after the final fresh-read admission point cannot recall entry

### Requirement: Exact same-scope refresh and request fencing
The native local grant SHALL preserve its item/vault/origin/repository/expiry
ceiling. The writer SHALL refresh the same opaque binding atomically only under an
exact old-version CAS with unchanged repository and expiry. Only uncommitted
uncancelled requests SHALL receive the refreshed binding while
preserving invocation identity. Every refreshed dispatch SHALL bind an exact signed
current request digest; committed/unknown work SHALL NOT gain new entry permission.

#### Scenario: Delayed old commit after rotation
- **WHEN** a waiting original invocation is rebound after native reconnection
- **THEN** old signed request digests and legacy digest-free commits are rejected
- **AND** its same invocation may dispatch once under the current binding

#### Scenario: Unknown and lost result acknowledgement
- **WHEN** provider entry has an uncertain outcome or receipt delivery loses its ACK
- **THEN** only its original unknown status or exact signed receipt is reconciled
- **AND** polling or native restart cannot enter the provider again

#### Scenario: Interrupted pending work and expired admission
- **WHEN** a native poll is cancelled after entry or a saved receipt awaits ACK
- **THEN** the original invocation SHALL settle Unknown or resend its exact receipt
- **AND** an expired committed-but-unentered request SHALL settle NotDispatched
- **AND** pending SHALL clear after delivery ACK, allowing later jobs without reconnect
- **AND** none of these states SHALL authorize another provider entry

### Requirement: Explicit snapshot boundary
Archive mode SHALL remain an explicit snapshot and SHALL NOT provide live freshness
or substitute for a disconnected live connection. Fixed GitHub metadata scope and
unsupported Google/general browser/signed distribution limitations SHALL remain.

#### Scenario: Live service unavailable
- **WHEN** the native live vault cannot be freshly validated
- **THEN** a historical archive does not authorize the live grant to dispatch
