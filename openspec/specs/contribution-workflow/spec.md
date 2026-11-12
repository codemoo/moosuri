## Purpose

Provide a small shared workflow for human and AI contributors using Git pull requests and OpenSpec, without repeated permission ceremonies or parallel conflicting sources of truth.

## Requirements

### Requirement: Proportionate changes
Contributors SHALL use OpenSpec for material behavior, security or compatibility changes and MAY submit small nonbehavioral fixes or compatible recipes through direct pull requests.

#### Scenario: Documentation typo
- **WHEN** a contributor corrects a typo without changing a contract
- **THEN** a direct pull request with appropriate verification is sufficient

#### Scenario: Credential scope change
- **WHEN** a change expands credential use or an external operation
- **THEN** its OpenSpec delta states the scope and observable failure checks

### Requirement: Safe contribution checks
Fork pull-request checks MUST use read-only repository access and synthetic data without production, provider or vault credentials.

#### Scenario: Untrusted contributor code
- **WHEN** a fork pull request runs checks
- **THEN** it receives no secret-bearing evaluation or publishing environment
