## ADDED Requirements

### Requirement: Resume sealed memory intent before unrelated classification

An explicit authenticated retry SHALL resume a previously sealed local memory
intent under the current run lease and existing commit checks before constructing
a model client or running unrelated classifiers. It SHALL preserve the original
intent identity and SHALL NOT call a model to replace the accepted intent.

#### Scenario: Prepared memory command survives restart

- **WHEN** a run stops after sealing a local memory intent and its owner retries after reopening the Store
- **THEN** the original intent completes once with zero replacement model calls, even when unrelated classification has not completed

### Requirement: Preserve existing replay and authority boundaries

Local memory resumption SHALL require the Store's sealed intent and current
session, run and target authority. The runner SHALL retain existing provider
checkpoint rejection and remote dispatch rules. It SHALL NOT interpret provider
checkpoint bytes as memory mutation authority.

#### Scenario: No accepted local command

- **WHEN** no sealed local memory intent exists
- **THEN** ordinary existing run admission, classification and replay rules remain in force
