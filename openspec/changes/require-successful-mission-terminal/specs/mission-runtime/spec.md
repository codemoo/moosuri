## ADDED Requirements

### Requirement: Successful mission terminal admission

The assistant SHALL admit a final mission proposal or clarification only from a
successfully completed model response, and SHALL execute only validated complete
tool continuations under the existing original owner and source authority.

#### Scenario: Valid candidate JSON inside an incomplete response

- **WHEN** an incomplete provider response contains valid JSON that cites an
  earlier successful calculation and matches the original completion checks
- **THEN** the mission SHALL remain incomplete without an admitted candidate or
  private report, retaining earlier receipts and observed usage.

#### Scenario: Refused response or unsuccessful clarification

- **WHEN** a refused response or incomplete response contains valid candidate or
  clarification JSON
- **THEN** the assistant SHALL NOT report success, create an input wait, or replay
  its text as a completed proposal after restart.

#### Scenario: Unsuccessful tool response

- **WHEN** a response marked incomplete or refused includes tool-call bytes
- **THEN** none of those calls SHALL execute and no automatic inference retry
  SHALL start.

#### Scenario: Unverified pending capsule from an older runtime

- **WHEN** a pending legacy mission capsule has no persisted verified terminal
  marker after upgrade or restart
- **THEN** the assistant SHALL stop with an unresolved outcome before replaying
  unfinished tools, accepting its proposal or creating an input wait, retaining
  completed earlier receipts and original budgets.

#### Scenario: Successful response and existing owner boundaries

- **WHEN** a complete response cites actual current-run receipts and satisfies
  the existing mission verification checks
- **THEN** ordinary verification SHALL continue with the original owner, account,
  source, deadline and call limits unchanged.

### Requirement: Ordinary explanation continuation routing

The assistant SHALL answer an explicit ordinary answer/explanation continuation
without an extra task-classifier call when no independent task/date cue or
selected/clarification target requires task interpretation.

#### Scenario: Explanation after a saved task receipt

- **WHEN** the user asks to continue an earlier answer or explanation after a
  saved-task receipt
- **THEN** the assistant SHALL use the ordinary answer path without creating or
  controlling a task merely because that receipt exists.

#### Scenario: Explicit task continuation

- **WHEN** the user names a task, selects its card or answers its original bounded
  clarification
- **THEN** the existing task interpretation and authority checks SHALL remain
  available.
