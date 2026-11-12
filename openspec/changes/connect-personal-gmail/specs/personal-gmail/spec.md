## ADDED Requirements

### Requirement: Personal connection ownership
Gmail connections MUST belong to the authenticated user. Administrators MUST NOT
read another user's tokens, inbox or draft content. OAuth completion MUST require
the initiating authenticated session, unexpired one-use state and PKCE. Tokens MUST
be encrypted and excluded from model input, ordinary tool output and public DTOs.

#### Scenario: Callback arrives without the login cookie
- **WHEN** Google returns an authorization code to the public callback
- **THEN** it is only staged behind an opaque handle
- **AND** an authenticated same-origin completion verifies the original session before exchange

#### Scenario: Refresh finishes after disconnect
- **WHEN** a connection is revoked while refresh or a mail read is in flight
- **THEN** the old connection epoch cannot be restored or publish new private output
- **AND** dependent pending drafts, checkpoints and inherited answers are withdrawn

### Requirement: Exact final user approval
Every email send MUST require explicit final user approval of the exact recipients,
content and attachments preview. Chat instructions, retrieved content, model calls
and skill text MUST NOT grant that approval. Approval MUST bind a frozen revision
and digest and MUST NOT accept replacement content in the approval request.

#### Scenario: Agent is asked to send an email
- **WHEN** the agent prepares the requested message
- **THEN** it creates a local pending draft with no send dispatch
- **AND** only the user's final confirmation can approve the displayed snapshot

#### Scenario: User edits a displayed draft
- **WHEN** editing begins or a save response is pending
- **THEN** approval of the previous preview is disabled in both UI and handler
- **AND** a changed snapshot is displayed and requires its own approval

#### Scenario: Open draft is withdrawn
- **WHEN** a newer action snapshot is purged, cancelled or otherwise invalidates the editor's revision
- **THEN** copied recipients and body are removed from the editor before the next paint

### Requirement: Durable truthful dispatch
The worker MUST commit one dispatch before sending frozen MIME bytes. A successful
result MUST include the provider receipt and MUST NOT imply recipient delivery or
reading. Unknown results MUST NOT be automatically resent. Late receipts MUST stay
associated with the original dispatch after cancellation or disconnect.

#### Scenario: Process stops after dispatch commitment
- **WHEN** startup finds an unfinished committed send
- **THEN** its outcome becomes unknown
- **AND** restart and ordinary chat retry issue no replacement send

### Requirement: Bounded read execution and revocation
Mail reads MUST run outside the SQLite actor, through the original admitted run
and current connection pin. Omitted sections MUST be explicitly incomplete. Local
revocation MUST share the existing SSE delivery fence, without holding it across
network IO. No generic browser action may bypass the email approval contract.

#### Scenario: Revocation overlaps a held delivery
- **WHEN** a disconnect or refresh failure withdraws mail-derived content while SSE delivery is held
- **THEN** revocation and delivery are ordered by the shared gate
- **AND** withdrawn plaintext is not queued after the revocation commits
