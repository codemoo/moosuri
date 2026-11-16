## ADDED Requirements

### Requirement: Ambiguous answers retain only the existing unresolved question
A bounded partial request MUST retain its established field across an accepted
ambiguous answer only while that field is unresolved. Original intent, operation,
complete values, literal answers, expiry and authority limits MUST remain unchanged.
Display MUST ask the established question without rewriting the acknowledged decision.

#### Scenario: Duration answer is corrected after reopen
- **WHEN** the owner asks to create one timed event, answers the duration question ambiguously and then supplies one literal duration after reopen
- **THEN** the original event title/date/start/operation and duration question remain available
- **AND** the correction produces one counted typed write with the original start and corrected duration
- **AND** a finished lease cannot repeat that write

#### Scenario: Original pending question expires
- **WHEN** the original pending context has expired before a correction arrives
- **THEN** the correction inherits no prior action request
- **AND** old request quotes cannot authorize an event from the bare new answer

### Requirement: Unresolved local timezone preserves the time question
A complete Calendar or local mission clock MUST retain an established time question
while its admitted timezone is missing. The existing IANA validator and frozen
metadata MUST determine that gap. Later browser metadata MUST NOT fill it silently;
Relative/Interval mission times MUST NOT gain a timezone requirement.

#### Scenario: Complete clock timezone is corrected after reopen
- **WHEN** a complete-clock Calendar event or local-time mission has no admitted timezone and the owner answers its time question ambiguously
- **THEN** the same time question and all complete typed clock/action fields remain pending through reopen
- **AND** one explicit valid IANA correction creates the original event or schedule once in that timezone
- **AND** changed browser metadata does not replace the frozen missing timezone

### Requirement: Whole pending-question cancellation is local and scoped
A whole supported literal cancellation reply to the immediate owned pending
question MUST complete locally and retire that question without inference.
The same request ID MUST replay the stored completion. Saved schedules, missions
and events MUST remain unchanged through reopen. Existing owner/profile/session,
media and selected-target guards MUST remain in force.

#### Scenario: Cancel then make a separate fresh request
- **WHEN** the owner replies `취소` or `됐어, 취소해 줘` to a pending question and later makes a separate complete event request
- **THEN** cancellation spends no model call or external write, including exact replay/reopen
- **AND** the later request receives its own current-user authority without the cancelled draft

#### Scenario: Cancellation text is not a whole generic reply
- **WHEN** the input names an event/task, quotes cancellation, makes it conditional or combines it with a new action
- **THEN** the local pending-question cancellation grammar does not consume it
- **AND** `cancel reminder` remains on the reminder-specific route
