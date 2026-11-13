## ADDED Requirements

### Requirement: Public connectors exclude private Kakao collection
The public distribution SHALL support an official adapter category and a generic
custom ingestion protocol, without shipping a Kakao collector, Kakao-specific
retrieval tool, installer or runtime dependency. The existing personal collector
SHALL remain an independently operated external producer.

#### Scenario: A user connects a personal collector
- **WHEN** the user configures a private producer using the generic custom protocol
- **THEN** moosuri accepts only the scoped protocol and does not install or run that producer
- **AND** the public adapter catalog does not claim Kakao support

### Requirement: Classification does not imply availability
The system SHALL distinguish official/custom classification from shipped code,
personal credentials/scope, enabled capabilities and validation evidence.

#### Scenario: Planned Slack and interactive Gmail
- **WHEN** an adapter has no implemented source-sync capability
- **THEN** neither its official category nor existing interactive mail tools mark source sync ready
- **AND** the user sees the actual supported and unavailable functions

### Requirement: Source commits retain connection provenance
The system SHALL bind source identity to authenticated tenant, connection and
immutable scope, and commit source revisions, provenance, batch receipt, local
sequence and opaque provider checkpoint together. Provider checkpoints SHALL be
separate from the existing local i64 collection sequence.

#### Scenario: Lost acknowledgement or colliding external IDs
- **WHEN** a producer repeats an accepted request or another connection uses the same external ID
- **THEN** an exact retry returns its original receipt while different connections retain separate source identity
- **AND** changed payloads or stale checkpoints do not advance data or cursors

### Requirement: Custom credentials grant only scoped ingestion
The system SHALL use bounded, revocable, expiring credentials scoped to one
connection's ingestion/status. It SHALL recheck current account/config/credential
epoch at commit and SHALL NOT accept a producer-selected tenant or official role.

#### Scenario: Revocation or forged category
- **WHEN** a revoked producer sends a delayed batch or claims another tenant/official adapter identity
- **THEN** no data, receipt or checkpoint is changed or disclosed
- **AND** no chat, original-source read, tool execution or email-send authority is granted

### Requirement: Sync reporting distinguishes observations and producer claims
The system SHALL distinguish heartbeat, local commit and remote-range coverage,
keep bounded run histories with generation fences and expose safe metadata in
connection/source views. Custom producer success SHALL be labelled as a report,
not proof of provider truth or completeness.

#### Scenario: Heartbeat, partial data and late success
- **WHEN** a producer sends only a heartbeat, a partial page or an old run's success
- **THEN** heartbeat does not advance the data checkpoint, missing records are not deleted and stale reports cannot overwrite a newer run
- **AND** the visible result does not overstate completeness

### Requirement: Source withdrawal survives external ingestion
The system SHALL retain source/memory/evidence deletion and disconnect keep/delete
semantics, reject incomplete snapshot retirement and avoid replay after recovery.

#### Scenario: Disconnect during a batch or interrupted snapshot
- **WHEN** a connection is revoked/disconnected or a snapshot ends before verified finalization
- **THEN** late commits cannot revive withdrawn data and partial omission deletes no records
- **AND** retained copies, deleted copies and forgotten memories preserve their distinct scopes
