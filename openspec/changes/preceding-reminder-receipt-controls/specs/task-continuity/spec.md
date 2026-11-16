## ADDED Requirements

### Requirement: Preceding fixed reminder receipt controls

The assistant SHALL resolve an explicit current deictic fixed-reminder control
from the immediately preceding actual receipt under current authority and
revision guards, without model interpretation or a global reminder fallback.

#### Scenario: Move and cancel the just-created reminder

- **WHEN** an owner creates a fixed reminder, then requests `그 알림` or `방금 그
  알림` be moved to an explicit time and cancelled in the same conversation
- **THEN** the original reminder identity SHALL be edited and then revoked with
  increasing receipt revisions, without a duplicate reminder, mission or model
  call, while unrelated reminders remain unchanged.

#### Scenario: Original request is replayed

- **WHEN** a successful move request is submitted again with its original UUID
  and payload
- **THEN** the assistant SHALL return its original run and receipt without another
  edit or revision.

#### Scenario: Continuity evidence no longer identifies a current target

- **WHEN** the prior receipt is from another thread, login or authentication
  epoch, exceeds thirty minutes, has an intervening turn or a stale revision
- **THEN** the assistant SHALL ask for an exact reminder or explicit selection and
  SHALL NOT mutate even a sole owned reminder or call a model.
