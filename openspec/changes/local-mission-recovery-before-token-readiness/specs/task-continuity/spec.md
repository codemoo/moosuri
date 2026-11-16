## ADDED Requirements

### Requirement: Local acknowledged mission recovery

The assistant SHALL complete guarded local recovery of an acknowledged final
mission proposal without requiring remote model-token discovery or refresh.

#### Scenario: Token expired while completed proposal awaits verification

- **WHEN** the original run has a verified completed proposal and actual local
  calculation receipt, its personal token has expired and OAuth discovery is
  temporarily unavailable after process restart
- **THEN** local verification SHALL finish that same mission without another
  inference, refresh, task identity or dispatch budget allocation.

#### Scenario: Authority withdrawn before local verification

- **WHEN** the original personal binding is disconnected or a current-session
  mission's original login is revoked before recovery
- **THEN** the assistant SHALL reject successful completion under the existing
  owner/binding/session guards, without another model call or token refresh.

#### Scenario: More inference is needed

- **WHEN** guarded local recovery does not finish the task and it needs another
  inference
- **THEN** the assistant SHALL resolve current personal authorization and token
  readiness before the existing guarded model dispatch.
