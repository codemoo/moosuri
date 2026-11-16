## ADDED Requirements

### Requirement: Shared bounded local Store retries
The agent SHALL use a shared bounded retry implementation for existing runner and
mail receipt operations while retaining each operation's error and side-effect contract.

#### Scenario: Local receipt admission is busy
- **WHEN** a local Store operation returns Busy
- **THEN** it may retry the same payload at most five total attempts with 20 ms
  between attempts, without repeating external IO.

#### Scenario: Local acknowledgment is unknown
- **WHEN** a local operation returns Unavailable, conflict, revocation or another non-Busy error
- **THEN** it returns that result immediately without replaying the operation.

#### Scenario: Current run authority is temporarily unavailable
- **WHEN** the existing same-lease authority check returns Busy or Unavailable
- **THEN** the bounded check may repeat while terminal revocation or budget errors
  stop it immediately, and no new dispatch authority is created.

#### Scenario: Mail send reply is lost
- **WHEN** an approved send has already reached the provider and its reply is lost
- **THEN** local receipt retries, reconciliation and restart do not submit another send.
