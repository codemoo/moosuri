## ADDED Requirements

### Requirement: Relative edits use the actual owned reminder
The system SHALL apply an explicit supported relative one-off reminder edit to
the saved current timestamp while preserving identity, target authority and replay.

#### Scenario: Postpone then cancel the previous reminder
- **WHEN** an owner creates tomorrow's10:00 reminder, says “방금 그 알림을 30분 늦춰줘.” and then “방금 그 알림은 취소해 줘.”
- **THEN** the same reminder changes to10:30 and is revoked, with monotonic revisions and no model dispatch
- **AND** another owned reminder remains unchanged

#### Scenario: Replay after restart
- **WHEN** the exact edit request is retried before or after restart
- **THEN** the existing run and receipt return without another time shift

#### Scenario: Ambiguous or invalid offset
- **WHEN** the target is recurring, the destination is past/out of validity, or the command is quoted, conditional or ambiguous
- **THEN** the system SHALL NOT apply a relative edit or claim it was saved
