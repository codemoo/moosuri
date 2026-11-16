## ADDED Requirements

### Requirement: One mission capacity policy
Mission admission, waits and resume SHALL use the same owner slot definitions
while preserving the distinction between new and existing work.

#### Scenario: New work at capacity
- **WHEN** sixteen outstanding missions already exist
- **THEN** a new mission is rejected without adding a carrier or consuming another slot.

#### Scenario: Existing work at capacity
- **WHEN** an existing task needs input and its owner has a waiting slot
- **THEN** it can wait and later resume within the same global place, preserving
  its request, completed work, accounting, deadline and profile.

#### Scenario: Paused owner and ready waits
- **WHEN** a paused task occupies an owner's runnable slot
- **THEN** ready waits remain queued in original admission order until the slot is
  released, and another owner's work remains independent.

#### Scenario: Native foreground chat
- **WHEN** foreground chat waits for a selected device's mail result
- **THEN** mission waiting limits are not imposed on that foreground run, and its
  existing device, session and continuation guards remain authoritative.
