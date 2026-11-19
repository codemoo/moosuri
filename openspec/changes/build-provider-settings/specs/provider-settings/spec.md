## ADDED Requirements

### Requirement: Shared provider ownership
Only an authenticated unrestricted administrator MUST create or edit shared connections. Only an authenticated unrestricted administrator MUST be able to change the installation default or manage an administrator-owned model connection. A configured installation default SHALL govern inference for every role. Historical member assignments SHALL have no effect.

#### Scenario: Member attempts to replace a shared key
- **WHEN** a member sends a provider mutation
- **THEN** the server rejects it and preserves the profile

### Requirement: Credential isolation
Provider credentials MUST be encrypted with an independent private installation key and MUST NOT appear in public response DTOs or error bodies. Keeping a key across a destination/config change MUST be rejected.

#### Scenario: Database ciphertext is swapped
- **WHEN** encrypted credential bytes are moved to a different profile or revision
- **THEN** authenticated decryption fails before any network request

#### Scenario: Installation key is missing
- **WHEN** an initialized provider-key installation loses its key file
- **THEN** startup fails rather than generating a replacement key

### Requirement: Verified model choices
Catalog discovery MUST remain separate from text-stream verification. A verified choice MUST bind the exact provider revision, model and effort and require completed nonempty text.

#### Scenario: Provider changes while a probe is pending
- **WHEN** an admin edits or disables that connection before the result is committed
- **THEN** the previous probe cannot activate the new revision

#### Scenario: Provider returns a partial stream
- **WHEN** text is received without successful protocol completion
- **THEN** the choice remains unverified

### Requirement: Stable user preference
User and installation selections MUST pin a verified revision. Invalid or disabled selections MUST NOT silently fall back to another provider or model.

#### Scenario: Selected connection is disabled
- **WHEN** the user loads settings or resolves a model
- **THEN** the preference remains visible but unavailable until explicitly reselected

### Requirement: Bounded discovery and probing
Catalog and probe calls MUST use registered destinations, bounded time/body/concurrency and safe error codes, and MUST revalidate current account and provider state before publishing results.

#### Scenario: Catalog exceeds one page
- **WHEN** discovery reaches the model/page limit
- **THEN** the response explicitly marks the catalog as truncated and does not follow arbitrary next URLs

### Requirement: Owner-scoped model selection and capacity

The system SHALL present distinct model and effort controls and SHALL pin the chosen supported pair for new chat runs. It SHALL expose provider-reported capacity with its actual scope and reset time when available.

#### Scenario: Member has a historical self-selected model
- **WHEN** the member signs in, chats, or calls a model preference or provider management API directly
- **THEN** new inference uses the administrator policy and the member cannot create or modify model connections or preferences

#### Scenario: Personal subscription or compatible endpoint
- **WHEN** an administrator connects their personal account or verifies an OpenAI-compatible model
- **THEN** that owner can select it, and other accounts and installation defaults cannot use its private binding

#### Scenario: Discover an installed local model
- **WHEN** the administrator starts local discovery on an admitted endpoint or pairs an administrator workstation through the SSH helper
- **THEN** discovery lists installed models, and an explicit selection verifies text before use

#### Scenario: The provider omits a limit
- **WHEN** an authenticated provider omits quota or reset information
- **THEN** settings show that the missing information is unavailable
