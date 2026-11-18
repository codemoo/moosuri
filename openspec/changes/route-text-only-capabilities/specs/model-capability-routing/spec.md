## ADDED Requirements

### Requirement: Capability-aware chat dispatch

The runner SHALL try the verified model-independent latest-inbox route before its text-only operation guard. When a clear tool-dependent request reaches an admitted text-only model and no direct route applies, the runner SHALL give a model selection action without invoking the provider. Ordinary text requests and tool-verified models SHALL retain their existing execution paths.

#### Scenario: Natural inbox paraphrase
- **WHEN** the owner asks colloquially which mail just arrived
- **THEN** the same latest-inbox lookup or setup guidance is used with each model connection

#### Scenario: Web lookup with a text-only selection
- **WHEN** an explicit web lookup reaches a text-only model
- **THEN** chat points to model settings and the server records zero provider dispatches

#### Scenario: Quoted instruction or drafting request
- **WHEN** a user asks to translate a quoted instruction or write a draft
- **THEN** the selected text model can answer the request

### Requirement: Exact capability in model settings

The model selector SHALL identify tool capability for the exact model and reasoning setting. If any combination for a newly chosen model has verified tools, the picker SHALL prefer a verified combination. A user SHALL be able to verify an eligible current combination with its existing owner or shared probe endpoint.

#### Scenario: Existing text proof and a verified alternate effort
- **WHEN** two reasoning settings have different verified tool capabilities
- **THEN** the settings distinguish the two and the user can choose either explicitly

#### Scenario: Selected official Claude subscription
- **WHEN** the active binding is the official Claude CLI text profile
- **THEN** the UI labels it text-only and links work blocked by model capability to the model setting
