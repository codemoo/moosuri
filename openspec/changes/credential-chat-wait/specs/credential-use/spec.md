## ADDED Requirements

### Requirement: Typed original chat credential request
The system SHALL admit one terminal GitHub metadata read from the current run's
typed model tool slot only when the exact repository is present in current user
text, current owner/session authority and the frozen experimental capability hold.
It SHALL reuse original run/step and retain one mapped call/nonce; no generic URL,
secret input, scope expansion or silent new invocation is allowed.

#### Scenario: Locked device and normal restart
- **WHEN** the original device is locked/offline or not yet connected
- **THEN** chat displays waiting plus its connection action, releases worker/model
  capacity and preserves the same job/slot/run through normal restart
- **WHEN** a valid compatible device connects and returns the signed result
- **THEN** the same run receives the fixed sanitized metadata answer without a new
  model plan, provider dispatch or repeated local scope selection

### Requirement: Existing deadline and cancellation fences
Waiting SHALL preserve the original run deadline and current authority. The system
SHALL distinguish unknown committed results from uncommitted expiry, never redrive
an old invocation, and prevent cancel/logout/delete/restore from resurrecting output.

#### Scenario: Cancel before a late receipt
- **WHEN** the user cancels a waiting/committed chat and its authentic receipt arrives
- **THEN** private original settlement remains valid but no chat result is resurrected
- **AND** explicit retry cannot create another credential invocation for that run

### Requirement: Explicit experimental capability
Default and preexisting run profiles SHALL remain without the credential tool. The
server feature SHALL propagate the capability explicitly to Store/agent admission.

#### Scenario: Old profile or unsupported plan
- **WHEN** an old/default profile, media, mission, mixed tool batch or artifact write
  tries to invoke the credential tool
- **THEN** no credential job/commitment is admitted and no token is requested in chat
