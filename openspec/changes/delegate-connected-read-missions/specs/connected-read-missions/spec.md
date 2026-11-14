## ADDED Requirements

### Requirement: Explicit connected read delegation
Only a current owner request naming Gmail/mail or Calendar reads SHALL create an
immutable read grant within the existing mission harness. Required unavailable
domains SHALL remain unavailable; accepted work SHALL NOT silently lose them.
Old profiles and personal-subscription foreground SHALL NOT gain background reads.

#### Scenario: Daily briefing from current evidence
- **WHEN** the owner delegates connected Gmail and primary Calendar reads
- **THEN** the actual mission Runtime uses fresh search/read/list/observation evidence
  for that occurrence within the original model/tool/time budget
- **AND** the result remains private and subject to owner semantic review

#### Scenario: Unsupported required effect
- **WHEN** the current request requires sending, drafting or Calendar writes
- **THEN** no effect is executed or silently omitted to save a reduced read task

### Requirement: Precise durable read carrier
A credential carrier SHALL derive only from a guarded started read permit and the
saved owner/account epoch/mode/parent/provider/connection/app pins. It SHALL NOT
contain a fabricated logged-in UserContext. Refresh SHALL use exact owner and
credential revision CAS without expanding scope.

#### Scenario: Logout and foreign account decoy
- **WHEN** a durable owner logs out before execution
- **THEN** original delegated reads may continue with only that owner's pins
- **AND** foreign connections are unavailable and their credentials/history are absent
- **AND** current_session work still requires its original actual session

### Requirement: Withdrawal and fresh occurrence fencing
Admission, IO and settlement SHALL recheck authority. Revocation/disconnect/pause/
provider change SHALL clear private dependent projections and block late results,
while minimal receipts and unknown outcomes remain and no blind retry is made.

#### Scenario: Disconnect in flight
- **WHEN** Calendar or Gmail disconnects after read dispatch but before response
- **THEN** late content cannot restore candidate, private receipts or checkpoints

#### Scenario: Reconnection and recurring scope
- **WHEN** a saved recurring pin is withdrawn and the owner reconnects
- **THEN** no fresh occurrence or resume silently repins; a new delegation is required
