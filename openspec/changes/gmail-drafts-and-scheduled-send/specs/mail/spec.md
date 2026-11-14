## ADDED Requirements

### Requirement: Explicit provider draft authority
The system SHALL offer real owner-scoped Gmail draft list/read/create/update/delete
only with explicit opt-in compose consent. A provider draft write SHALL NOT grant
send approval or mutate an already frozen local send snapshot.

#### Scenario: Default connection or partial consent
- **WHEN** a default connection or an incomplete compose consent is used
- **THEN** draft IO is unavailable and no provider draft request is made

#### Scenario: Gmail changes after local approval
- **WHEN** a provider draft is edited or deleted outside the local action
- **THEN** the approved local bytes remain unchanged
- **AND** a stale provider copy cannot silently overwrite a detected newer version

#### Scenario: Unsupported MIME or lost write response
- **WHEN** the provider draft cannot be faithfully edited or a write response is lost
- **THEN** no unsupported parts are silently dropped and no write/send is automatically retried

### Requirement: Fixed one-off timing is part of final approval
The system SHALL display exact content, recipients, UTC instant and IANA/local time
before every scheduled send approval. Timing/content edits SHALL revoke approval.
Due execution SHALL use only the original immutable approved MIME with current
account, epoch, connection, source and background grant fences.

#### Scenario: Before due, missing approval or changed timing
- **WHEN** due time has not arrived, approval is missing, or a timing revision changes
- **THEN** dispatch commitments and provider sends remain zero

#### Scenario: Logout, revocation and expiry
- **WHEN** logout occurs alone
- **THEN** separately approved scheduled work may remain active
- **WHEN** account eligibility/epoch, connection, source or pinned grant is invalid or due+24h expires
- **THEN** no new dispatch is authorized

#### Scenario: Restart or uncertain scheduled result
- **WHEN** a process stops after a scheduled send commitment or loses its response
- **THEN** the result becomes unknown and no automatic resend occurs
- **AND** existing read-only reconciliation and original late receipt rules apply
