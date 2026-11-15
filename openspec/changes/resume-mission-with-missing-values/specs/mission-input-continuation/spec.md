## ADDED Requirements

### Requirement: Typed values continue the original admitted task
The system SHALL persist a finite typed missing-value question on the original mission card, release runnable capacity while unanswered, and accept an authenticated owner reply for the exact wait identity. A reply SHALL preserve the original mission/run, acceptance revision and floor, completed receipts, pinned profile, counters and deadline. A reply SHALL NOT grant a new operation or replace an existing complete value.

#### Scenario: Completed reads survive a missing recipient and restart
- **WHEN** an admitted one-off mail task completes reads and lacks its recipient
- **THEN** the original card asks for one email address and performs no model polling while unanswered
- **AND** after restart and a valid answer, it uses the completed receipts and prepares one original MailAction
- **AND** final mail approval remains required with no automatic send

#### Scenario: Replies and ready work retain their boundaries
- **WHEN** the same reply UUID is retried after acknowledgement loss
- **THEN** the current saved result is returned without another transition or action
- **AND** a conflicting payload, wrong owner, stale wait, expired grant or unsupported field fails
- **AND** ready tasks join the existing admitted-order capacity promotion

#### Scenario: Restore cannot replay saved answers
- **WHEN** a snapshot with a ready input answer is restored
- **THEN** input authority is cleared and unfinished mission grants are quarantined
- **AND** neither model nor external action automatically resumes
