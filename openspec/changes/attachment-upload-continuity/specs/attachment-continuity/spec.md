## ADDED Requirements

### Requirement: Independent prepared-image admission
Image bitmap decoding and resizing MUST remain serialized, but a pending upload receipt MUST NOT block preparation or upload of another selected image. Sending a ready selection MUST preserve other pending selections and the text draft under existing limits and immutable attachment identities.

#### Scenario: First upload stalls
- **WHEN** the first selected image has finished preparation but its upload response remains pending
- **THEN** the second image can prepare and upload, and sending that ready image leaves the first upload and next text intact

### Requirement: Owned attachment inventory refresh
Only the latest current-scope library refresh MUST publish its inventory or error. Superseding refreshes and scope cleanup MUST cancel prior read transports without changing upload recovery IDs or replaying writes.

#### Scenario: Old inventory arrives after ready upload inventory
- **WHEN** a new inventory exposes a ready attachment before an older retry_required inventory response arrives
- **THEN** the ready attachment and Attach control remain, and the superseded read cannot replace them
