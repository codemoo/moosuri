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
