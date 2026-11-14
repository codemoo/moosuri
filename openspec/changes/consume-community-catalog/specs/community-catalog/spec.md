## ADDED Requirements

### Requirement: Configured public discovery with verified offline cache
The runtime SHALL consume only the operator-configured normalized public GitHub catalog source, on cards access at a daily interval with ETag and bounded manual refresh. It SHALL persist and revalidate public cache bytes/digest/timestamps with source-scoped lease/generation, preserving the last valid offline index and installations while failing closed on corruption. It SHALL NOT send private queries/context or add LLM calls, background polling, private authentication or arbitrary endpoints.

#### Scenario: Offline or concurrent refresh
- **WHEN** a refresh fails, repeats, is interrupted or completes after lease/source generation replacement
- **THEN** previous verified discovery/install state remains, stale status is honest, and stale publication fails without user activation.

### Requirement: Selected artifact and original-source verification
Only user-selected packages SHALL be fetched. Index metadata SHALL remain preview-required until artifact SHA256, strict v2 format, exact source identity/content digest and original public GitHub source bytes are independently verified through the existing adapter, or an independently verified immutable exact snapshot is already cached. Index/artifact labels SHALL NOT construct verified-public-source provenance. Existing per-user lifecycle and capability validation SHALL own activation.

#### Scenario: Moving artifact or forged source
- **WHEN** an artifact changes under a moving catalog ref, claims a different source/content identity, or source reacquisition fails
- **THEN** preview/activation fails and the prior installed version remains.

#### Scenario: Explicit activation and stable preference
- **WHEN** one user previews, adds, updates, rolls back, hides, disables or removes a package
- **THEN** only that user's state changes, preview/refresh never executes work, and stable hide/tombstones survive catalog versions.

### Requirement: Honest existing-card experience
Discovery SHALL use existing Suggestions cards with actual connection readiness only after content verification, preserve installed prior-version actions, place source/version/digest diagnostics in details, and distinguish offline/unconfigured/unavailable state. It SHALL NOT claim execution readiness from index metadata, automatically install/update/run/mail, or expand native capabilities.

#### Scenario: Preview required for an uninstalled offer
- **WHEN** the configured catalog contains a package whose content has not been verified
- **THEN** its card offers content and origin verification before installation and does not claim execution readiness
- **AND** installed prior-version actions remain available, source/version/digest diagnostics stay in details, and discovery status distinguishes offline, unconfigured and unavailable states.
