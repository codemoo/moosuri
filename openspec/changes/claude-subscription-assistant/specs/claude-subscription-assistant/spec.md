# Claude subscription assistant

## ADDED Requirements

### Requirement: Verify tool use for the selected subscription configuration

Moosuri SHALL verify and store host tool capability against the owned Claude binding revision, concrete model, reasoning level, and bridge version. Text verification SHALL remain valid independently.

#### Scenario: The exact model consumes a real tool result

- **WHEN** the selected Claude model requests the synthetic verification tool and returns the fresh value supplied in its tool result
- **THEN** new turns on that exact configuration can use the common foreground tool runtime

#### Scenario: A proof is interrupted, malformed, or belongs to another selection

- **WHEN** either inference lacks a complete verified termination or fails to match the configuration or result
- **THEN** Moosuri does not enable a new tool capability or replace the user's selection

### Requirement: Execute foreground tools through the host

The Claude CLI SHALL emit bounded decisions through its existing isolated connection. Moosuri SHALL retain ownership of tool authorization, dispatch, receipts, and handling cancellation. Mail delivery SHALL require the existing explicit final preview approval.

#### Scenario: The assistant needs an actual result to answer

- **WHEN** a verified Claude configuration returns an admitted tool decision
- **THEN** Moosuri runs the typed operation and returns the actual receipt to the same model for continuation

#### Scenario: An invalid or revoked decision arrives

- **WHEN** the bridge response is incomplete, invalid, or no longer authorized
- **THEN** Moosuri does not dispatch new effects from that decision or publish the decision envelope as chat content
