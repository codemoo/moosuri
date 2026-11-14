## ADDED Requirements

### Requirement: Exact native GitHub credential use
The system SHALL execute only the typed GitHub repository metadata GET through a
trusted native adapter using a locally selected authenticated API-key vault item.
The exact local origin/operation/repository ceiling SHALL reject server expansion.
No secret/master/token SHALL reach assistant storage, model output, argv or journal.

#### Scenario: Compatible reuse and local lock
- **WHEN** another explicit request uses the selected repository
- **THEN** the original compatible local grant is automatically reused
- **WHEN** the native archive is locked or offline
- **THEN** durable original work waits without model polling until unlock/connect

### Requirement: Durable authenticated request and result lifecycle
The system SHALL pin original invocation/device/grant identity, journal preparation
and entry, acquire current online writer commitment, and settle an authenticated
allowlisted numeric/boolean projection. No arbitrary URL/header/body is accepted.

#### Scenario: Lost ACK, restart and stale authority
- **WHEN** execution or delivery is interrupted
- **THEN** only the same retained receipt/identity is reconciled, and no second
  adapter entry is permitted after restart
- **WHEN** account/binding authority is revoked before commitment
- **THEN** provider entry is denied

### Requirement: Honest experimental boundaries
The product SHALL disclose archive-snapshot freshness/rotation limits, absent
signed distribution and unsupported Google/chat-tool adapters. The default server
SHALL NOT expose experimental credential routes.

#### Scenario: Unavailable feature
- **WHEN** the default server opens the connection page
- **THEN** it displays the unavailable experimental state without claiming connected
