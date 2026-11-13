## ADDED Requirements

### Requirement: Unknown send reconciliation is bounded read authority
The system SHALL persist original frozen equality evidence before payload purge and SHALL keep unknown send state separate from read-only Sent observations. A lookup SHALL never create send authority or replay a committed request.

#### Scenario: Accepted send loses its response
- **WHEN** a provider accepts the exact frozen POST and drops its response
- **THEN** the actual MailService records unknown and performs only bounded GET lookups; repeated ticks, manual check and restart do not send again

#### Scenario: Search is insufficient proof
- **WHEN** there are zero, multiple or incomplete results, wrong metadata, missing Bcc, extra headers/parts, unsupported MIME, or unequal complete content
- **THEN** the action remains unknown with an honest unresolved/candidate status

#### Scenario: Legacy evidence is unavailable
- **WHEN** an old unknown action has no pre-purge equality oracle
- **THEN** fetched candidates do not become their own expected fingerprint and no equality observation is invented

#### Scenario: Authority changes during lookup
- **WHEN** account/auth/connection/scope/app authority changes, source is withdrawn, or lease CAS is stale
- **THEN** fetched private data and late observations cannot publish or resurrect credentials

#### Scenario: Restore and late synchronous receipt
- **WHEN** a snapshot is restored
- **THEN** all lookup leases and due authority are quarantined, and historical send receipts remain historical facts
- **WHEN** an original send-response receipt arrives after a lookup
- **THEN** it wins without overwriting conflicting lookup evidence or granting another send
