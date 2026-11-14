## ADDED Requirements

### Requirement: Exact personal foreground capability proof

The application SHALL enable personal foreground tools only after a successful bounded two-round tool/result challenge for the current owner, binding revision, model and effort, separate from catalog and text proof.

#### Scenario: Text success does not grant tools

- **WHEN** catalog or text verification succeeds
- **THEN** function tools SHALL remain disabled until the exact tuple passes tool verification.

#### Scenario: Verification authority expires

- **WHEN** owner session/epoch or binding revision changes during verification
- **THEN** no late tool proof SHALL commit and shared-provider admin authorization SHALL remain unchanged.

#### Scenario: Capability is verified after text admission

- **WHEN** a previously admitted text-only run's tuple later passes tool verification
- **THEN** that frozen run SHALL remain text-only while new turns may acquire the verified capability.

### Requirement: Durable plan foreground continuation

The application SHALL reuse the existing bounded Store tool loop, pinning plan dialect, model, effort and namespace in private continuation and resolving current personal access before every model round.

#### Scenario: Saved tool result is submitted

- **WHEN** a complete namespaced call is received
- **THEN** its checkpoint and typed call SHALL commit before execution and its saved result SHALL replay only through the matching plan continuation.

#### Scenario: Cancellation or revocation races a result

- **WHEN** the run, session, connection or evidence authority is revoked
- **THEN** late tool/output commits and subsequent model dispatch SHALL be fenced without automatic inference or external tool replay.

### Requirement: Existing foreground effects boundaries

Personal foreground tools SHALL retain the existing owner-scoped read, skill, arithmetic, artifact/Calendar read, local mail draft and web/browser contracts.

#### Scenario: Mail send is requested

- **WHEN** personal chat prepares a reply or the user asks to send
- **THEN** it SHALL create only a local unapproved draft and require the existing full frozen final preview and explicit owner approval before dispatch.

#### Scenario: Browser human control is used

- **WHEN** a human takes browser control and resumes the same conversation
- **THEN** existing owner/thread and human-control restrictions SHALL apply without model click/type/credential or approval-bypass authority.

### Requirement: Staged capabilities remain explicit

The foreground increment SHALL preserve personal attachment/media/background exclusions and SHALL NOT enable conversational memory/artifact/Calendar mutation planners.

#### Scenario: Earlier raw attachment context exists

- **WHEN** a personal turn includes current or inherited ready image/document dependencies
- **THEN** admission and durable dispatch fences SHALL reject it before private raw body loading or plan IO without changing the explicit personal selection.

#### Scenario: Typed mutation or mission is requested

- **WHEN** personal foreground chat asks for persistent memory/artifact/Calendar change or autonomous delegation
- **THEN** this increment SHALL report that boundary and SHALL NOT create a mutation grant or personal background execution.
