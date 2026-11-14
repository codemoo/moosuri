## ADDED Requirements

### Requirement: Personal Calendar-only interpretation

Exact personal tool-verified foreground turns SHALL reuse the current-user Calendar envelope while permitting only Calendar decisions or a fully-null no_command sentinel.

#### Scenario: Mission command or control is proposed

- **WHEN** a personal classifier proposes mission creation, scheduling or existing-task control
- **THEN** Store SHALL reject it at recording and effect preflight/application without mission or Calendar mutation.

#### Scenario: Prior run has no envelope

- **WHEN** a historical personal run did not admit a Calendar interpretation envelope
- **THEN** it SHALL NOT acquire Calendar write authority merely because the runtime was upgraded.

### Requirement: Current owned Calendar action

Personal Calendar chat SHALL retain existing primary-calendar timed solo-event create, same-day update and delete contracts without an added approval or generic write tool.

#### Scenario: Valid current event command

- **WHEN** the current owner supplies one complete supported event request
- **THEN** isolated plan interpretation SHALL use frozen model/effort and Calendar pins, and the existing typed service SHALL perform at most one admitted action and show its actual receipt.

#### Scenario: Unsupported or ambiguous event shape

- **WHEN** the request needs attendees, recurrence, all-day/secondary-calendar writes, ambiguous target/time or deferred execution
- **THEN** it SHALL clarify or report unsupported without creating a reduced task or performing a write.

#### Scenario: No connected Calendar exists

- **WHEN** a complete current event command has a frozen Calendar profile with no connection pins
- **THEN** the existing typed receipt SHALL report connection_needed with settings guidance and no action, and later connection SHALL require a new request.

### Requirement: Durable authority and no resend

Calendar planner dispatch SHALL resolve current personal access before durable admission, and sealed Calendar recovery SHALL precede fresh model-client construction while preserving the original UUID and authority.

#### Scenario: Sealed command is retried

- **WHEN** a valid sealed Calendar command is explicitly retried
- **THEN** it SHALL resume or render its existing receipt without fresh classification or replacing its action identity.

#### Scenario: Provider outcome is unknown

- **WHEN** dispatch loses its acknowledgement or later reload cannot establish success
- **THEN** the action SHALL retain uncertainty and allow only the existing same-ID lookup rather than resend.

#### Scenario: Cancellation or connection authority is withdrawn

- **WHEN** the run, original session, personal binding or Calendar connection/app is revoked during interpretation or execution
- **THEN** late private grants/results SHALL remain fenced, any dispatched effect SHALL retain only its allowed minimum receipt, and Retry SHALL NOT revive the withdrawn command.

### Requirement: Other personal capabilities remain bounded

Calendar foreground support SHALL NOT grant personal background missions, mission controls, media or mail-approval authority.

#### Scenario: Calendar support is enabled

- **WHEN** a personal foreground Calendar command becomes available
- **THEN** existing mission/media fences and final explicit mail preview approval SHALL remain unchanged.


### Requirement: Personal disconnect retires unfinished Calendar effects
The system SHALL atomically withdraw unfinished Calendar commands frozen to the disconnected personal binding and revision while retaining the minimum original external-effect receipt. It SHALL preserve already completed Calendar history according to existing account and Calendar withdrawal rules.

#### Scenario: Disconnect while a sealed command resumes a committed Calendar write
- **WHEN** the original sealed personal Calendar command resumes and its committed provider response is held while the exact personal binding is disconnected
- **THEN** the run becomes terminal and its private payload, event, observations and output are retired
- **AND** a late applied or unknown response retains only the original minimum effect receipt
- **AND** reconnection or Retry does not restore the retired command, private results or write

#### Scenario: Personal model disconnect follows a completed Calendar write
- **WHEN** a personal Calendar command completed before its personal model binding is disconnected
- **THEN** its completed conversation text, minimum receipt and private event remain governed by the existing history contract
- **AND** model disconnect alone does not delete that completed history
