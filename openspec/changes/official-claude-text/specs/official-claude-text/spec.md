## ADDED Requirements

### Requirement: Official owner-isolated text execution

The application SHALL offer official Claude Code as an optional unchanged external
executable, with owner binding/revision/profile and exact verified model selection.
It SHALL NOT extract or relay consumer credentials into its HTTP ModelPort. The
base application SHALL remain usable without the external runtime.

#### Scenario: Owned saved conversation

- **WHEN** an authenticated owner selects their exact probe-verified text choice
- **THEN** the ordinary durable Runtime commits dispatch before child input,
  persists validated text/usage, and restores the saved conversation on reconnect
- **AND** tools/media/background/catalog/observed-effort capabilities remain absent.

### Requirement: Discoverable connection setup

My model SHALL always expose a Claude connection entry. It SHALL distinguish
personal subscription runtime setup from administrator-managed Anthropic API
credentials, and SHALL NOT present a missing or failed runtime status as a
successful login. Existing owned bindings SHALL remain removable without a
configured runtime, retaining the credential retirement contract.

#### Scenario: Runtime not configured or temporarily unreachable

- **WHEN** an owner opens My model without a configured runtime or a successful status response
- **THEN** Claude connection setup remains discoverable, the missing setup or retryable error is explicit, and subscription creation/probing stays unavailable
- **AND** administrators can open the real shared Anthropic editor while members cannot access shared credential editing.

#### Scenario: Runtime removed after a connection exists

- **WHEN** the operator removes the optional runtime
- **THEN** the owner can still view and disconnect existing bindings, with local cleanup shown as pending until acknowledged.

### Requirement: Conservative process lifecycle

Execution SHALL use the pinned isolated Linux profile, bounded inert composed
input and independent HOME/config/cwd. Completion SHALL require validated terminal
protocol, clean EOF and successful process exit. Dispatched work without complete
evidence SHALL remain unknown and SHALL NOT be automatically replayed.

#### Scenario: Partial result and process loss

- **WHEN** output is partial or the final result is lost/malformed/nonzero-exit
- **THEN** partial observations remain available, accounting stays actual or unknown,
  and restart does not launch another inference for that invocation.

### Requirement: Credential retirement acknowledgement

Disconnect/delete SHALL fence new work and stale native login, durably record
owned credential-environment retirement, and retain pending cleanup until local
filesystem removal is acknowledged. Local deletion SHALL NOT imply remote revoke.

#### Scenario: Account deletion with pending credentials

- **WHEN** account content is purged while the optional runtime or filesystem cleanup is unavailable
- **THEN** only opaque retirement metadata remains, local deletion completion stays
  pending, and no old binding is selectable or executable.

### Requirement: Session-owned official subscription login bridge
The system SHALL bridge the unchanged official CLI authorization URL and returned one-time code to its original process, scoped to the current owner session and binding revision, without persisting raw output, URLs, codes or bearer tokens.

#### Scenario: Complete official login from settings
- **WHEN** the owner starts subscription login and returns the code from the official page
- **THEN** the code reaches only that original process once, and official success remains separate from exact-model verification

#### Scenario: Stale requests and session withdrawal
- **WHEN** code submission or cancellation supersedes an earlier status response, or the originating session expires or is revoked
- **THEN** stale UI responses cannot revive the prior code screen and withdrawn authority cancels the process while holding the environment lock through cleanup
