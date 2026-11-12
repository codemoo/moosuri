## ADDED Requirements

### Requirement: Provider-specific completion
A model adapter MUST distinguish completed, incomplete, refused, continuation-required and failed output and MUST NOT treat EOF as completion without its protocol terminal markers.

#### Scenario: Connection ends after partial answer
- **WHEN** text arrives but the protocol terminal marker does not
- **THEN** the caller retains delivered partial output and receives a truncated-stream failure without a successful completion event

#### Scenario: OpenRouter emits final accounting
- **WHEN** a content-free final usage frame repeats the prior finish reason before [DONE]
- **THEN** usage is recorded without a duplicate completion or protocol rejection

#### Scenario: Final snapshot contradicts delivered text
- **WHEN** a Responses done/final snapshot contains text different from the received deltas
- **THEN** the adapter reports a protocol failure instead of publishing successful completion

### Requirement: Bounded cancellable transport
Transport MUST bound bytes, frames, output, time and pending delivery, and cancellation or a closed consumer MUST promptly stop local work without claiming remote cancellation or replaying the request.

#### Scenario: Receiver closes during a stalled response
- **WHEN** the event receiver closes while response headers or body are held
- **THEN** local transport terminates promptly and no replacement model request is sent

### Requirement: Explicit capability and routing
Text encoding MUST preserve the selected model and verified effort, reject unsupported content/capability and keep reasoning separate from user-visible answer events. Automatic provider/model fallback MUST remain disabled.

#### Scenario: Local model effort is unverified
- **WHEN** the request selects an unsupported effort
- **THEN** it fails before any network request instead of relying on a provider default

### Requirement: Credential destination boundary
Hosted credentials MUST use fixed official destinations. Compatible endpoints MUST validate and pin their registered destination, reject prohibited metadata targets, preserve TLS verification and disable redirects and inherited proxies.

#### Scenario: Model endpoint redirects to another host
- **WHEN** the endpoint returns a redirect
- **THEN** the caller receives that HTTP failure and neither the prompt nor its credential is forwarded

### Requirement: Safe transport failures
Failures MUST expose bounded safe categories without provider bodies, prompt text or credential values, while missing token counters MUST remain unknown.

#### Scenario: Upstream error echoes private request text
- **WHEN** the error body includes request content or credentials
- **THEN** the transport error does not copy that body into logs or user-visible error output
