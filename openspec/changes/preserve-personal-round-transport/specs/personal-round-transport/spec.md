## ADDED Requirements

### Requirement: Personal foreground transport continuity

The application SHALL preserve an explicit run-local HTTP transport through
personal foreground planners and continuation rounds while resolving current
owned authorization and token readiness before every provider admission.

#### Scenario: Unchanged authorization across rounds

- **WHEN** a personal run completes a tool round and submits its result to the
  same permitted origin with unchanged validated DNS addresses
- **THEN** the next request SHALL be eligible to reuse that run's connection
  without bypassing its current RunLease, binding or DNS checks.

#### Scenario: Access-token rotation between rounds

- **WHEN** durable readiness refreshes a personal access token between rounds
- **THEN** the next request SHALL use the replacement bearer while remaining
  eligible to reuse the run's already validated connection.

#### Scenario: Owner authority or binding is withdrawn

- **WHEN** cancellation, account-epoch change or disconnection invalidates an
  in-flight run before continuation admission
- **THEN** no continuation request SHALL be sent using its cached transport.

#### Scenario: Another run constructs a client

- **WHEN** another independent client uses the same origin
- **THEN** it SHALL NOT inherit the preceding run's connection or credentials.

### Requirement: Undispatched refresh cleanup

The application SHALL release a matching acquired refresh admission after
provider discovery failure or caller drop without resetting a dispatched exchange.

#### Scenario: Discovery fails or its caller disappears

- **WHEN** discovery fails or is cancelled before token exchange dispatch
- **THEN** a prompt authorized retry SHALL be able to acquire readiness without
  waiting for the abandoned admission's 45-second expiry.

#### Scenario: Cleanup is stale or dispatch has already happened

- **WHEN** cleanup targets an old nonce or a dispatched rotating exchange
- **THEN** it SHALL leave the current admission untouched and SHALL NOT authorize
  replay of the old refresh token.

#### Scenario: Acquisition acknowledgement is lost

- **WHEN** a caller disappears before the store actor executes or delivers its
  acquired refresh result
- **THEN** the result's cleanup carrier SHALL release only that acquired nonce,
  allowing the next authorized attempt without waiting for lease expiry.

#### Scenario: Cleanup meets a full actor mailbox

- **WHEN** cleanup must wait for mailbox capacity and the last Store is dropped
- **THEN** cleanup SHALL NOT retain a strong mailbox sender across suspension or
  prevent the writer from closing and joining.
