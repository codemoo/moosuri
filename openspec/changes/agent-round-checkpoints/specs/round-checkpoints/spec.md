## ADDED Requirements

### Requirement: Shared received-round persistence
Foreground, mission, planner and Claude execution SHALL persist a received model
round with one bounded local retry policy, without repeating model or tool IO.

#### Scenario: Temporary writer pressure
- **WHEN** the Store reports Busy while saving an already received round
- **THEN** the same lease, bytes and tool slots are retried within the existing
  local limit, and a successful acknowledgment permits the original next step.

#### Scenario: Reopen after persistence
- **WHEN** a mission restarts after its round was saved but before its tool ran
- **THEN** it uses the saved slot and original accounting, executes the unfinished
  tool once and requests only the next necessary model response.

#### Scenario: Cancellation wins before retry
- **WHEN** an owner cancels the task before a delayed write is retried
- **THEN** the Store's lease guard rejects the write and restart cannot revive the
  task or execute its proposed tools.

#### Scenario: Unknown acknowledgment or permanent failure
- **WHEN** persistence returns Unavailable, a revoked lease or a permanent error
- **THEN** the helper returns that error without replay and the original caller
  retains its existing error settlement.
