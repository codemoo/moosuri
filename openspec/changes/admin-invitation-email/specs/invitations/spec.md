## ADDED Requirements

### Requirement: Separate immutable invitation preview
The system SHALL keep local account creation and one-time password copying usable
without a sender. It SHALL prepare a current-account-derived admin-only encrypted
snapshot and require explicit final approval of all recipients/content before each
actual invitation email. The model SHALL never receive temporary credentials or
an invitation approval/send tool.

#### Scenario: Account exists but mail is absent or fails
- **WHEN** the optional system sender is absent, fails or returns an unknown result
- **THEN** the local account and password-copy path remain available
- **AND** status never claims recipient delivery or retries sending automatically

#### Scenario: Stale actor or target credential
- **WHEN** the originating admin session, sender pin or target revision/epoch/status/expiry changes
- **THEN** old approval cannot dispatch
- **AND** reset/deletion clears pending sensitive snapshots

### Requirement: One durable invocation per credential effect
The system SHALL bind an immutable invitation/effect UUID, target credential epoch
and frozen MIME digest before delivery. It SHALL persist commitment and clear pending
payload before one bounded native invocation and SHALL never automatically resend
an unknown or committed effect after lost ACK, duplicate requests or restart.

#### Scenario: Duplicate approval and response loss
- **WHEN** approval is repeated or its response is lost
- **THEN** lookup by the original ID reports existing status without another send
- **AND** unknown remains unknown without read scope

#### Scenario: Optional native adapter privacy
- **WHEN** an operator explicitly configures a supported native gws sender
- **THEN** dedicated credential paths and a cleared environment isolate ambient auth
- **AND** MIME/password never appears in argv/logs and transient staging is private and cleaned
