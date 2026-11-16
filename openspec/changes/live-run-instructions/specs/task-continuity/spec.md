## ADDED Requirements

### Requirement: Live current-owner guidance receipts

The assistant SHALL durably acknowledge supported additional current-owner
guidance without inference and apply it at a safe boundary of the same task under
its original authority, receipts and finite budget.

#### Scenario: Owner adds guidance while a stream is active

- **WHEN** the owner submits a new valid UUID, exact text and current attempt on
  a supported active task
- **THEN** the assistant SHALL immediately return a received receipt without a
  model call and SHALL include its text as a current user message at the next
  safe inference boundary of the same run or mission.

#### Scenario: ACK outcome is uncertain

- **WHEN** the owner looks up or explicitly replays the original UUID and payload
- **THEN** the assistant SHALL return its current durable receipt; a mismatched
  payload SHALL conflict and SHALL NOT allocate another instruction or task.

#### Scenario: New guidance races with an old final or plan

- **WHEN** guidance is received before the old result is transactionally accepted
- **THEN** the assistant SHALL continue with that guidance under the same original
  budget and completed tool receipts, without accepting the stale result or
  replaying completed tools.

#### Scenario: Round cannot validly apply the guidance

- **WHEN** the relevant stream refuses, is malformed/incomplete, loses authority,
  exhausts its original budget or terminates before application
- **THEN** the assistant SHALL close received guidance unapplied with a fixed
  reason and SHALL NOT claim successful task completion from that output.

#### Scenario: Unsupported effect path or different authority

- **WHEN** the path is a typed local action, native credential/effect stage,
  unsupported plain transport, private-report/original Mail/Calendar effect mission or the
  caller has a different owner/epoch or lacks the original login for a foreground
  or current-session task
- **THEN** the assistant SHALL reject instruction admission; additional text
  SHALL NOT grant another operation or allow an obsolete write after ACK.

#### Scenario: Durable task owner logs in again

- **WHEN** a currently validated same owner logs in again at the original epoch
  and the durable mission's existing grant remains valid
- **THEN** the assistant SHALL admit guidance and exact receipt replay under that
  durable authority, retain the submitting login as audit data and SHALL NOT
  require the expired/revoked originating login.

#### Scenario: Follow-up in the original conversation

- **WHEN** a later user turn projects the completed effective prior turn
- **THEN** its model history SHALL include applied guidance once, preserve the
  displayed original text and exclude received/closed instructions.

#### Scenario: Deletion and restore

- **WHEN** the original task thread is deleted or a sealed restore is quarantined
- **THEN** its private guidance SHALL respectively cascade away or close unapplied
  and its prior readiness SHALL NOT authorize automatic execution.

### Requirement: Avoid incidental duration classification

The assistant SHALL distinguish quantified duration cues from incidental Korean
noun substrings before scheduling-intent inference, without granting any action.

#### Scenario: An ordinary question contains a duration-like substring

- **WHEN** an ordinary question contains 기분에, 성분에 or unquantified 수업 시간에
- **THEN** it SHALL proceed directly to the answer path without a scheduling model
  round; actual numeral periods and explicit recurring instructions SHALL retain
  semantic classification and current-owner validation.
