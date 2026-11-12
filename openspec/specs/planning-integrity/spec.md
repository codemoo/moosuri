## Purpose

Keep the public planning package internally consistent and distinguish verifiable repository artifacts from future product behavior. Preserve immutable source evidence while allowing reviewed decisions to evolve.

## Requirements

### Requirement: Planning verification
The repository SHALL validate source inventory coverage, local links, requirement and feature IDs, and scenario references without claiming product correctness.

#### Scenario: Missing feature coverage
- **WHEN** a required feature has no valid evaluation scenario mapping
- **THEN** the planning verifier fails and identifies the missing mapping

### Requirement: Honest completion
Contributors MUST distinguish implemented artifacts, executed checks, planned tests and unsupported product behavior.

#### Scenario: Plan exists without runtime
- **WHEN** a capability has a specification and no implementation
- **THEN** status remains planned and no runtime success is claimed
