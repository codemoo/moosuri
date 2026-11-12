## Purpose

Evaluate the assistant through observable user outcomes with bounded, reproducible synthetic environments and report capability, failures, costs and latency without inflated completion claims.

## ADDED Requirements

### Requirement: Independent mission oracle
The evaluator SHALL grade required postconditions, forbidden effects and provider receipts independently of the agent's success claim.

#### Scenario: Unsent email claimed complete
- **WHEN** the agent claims a message was sent but the provider has no successful receipt
- **THEN** mission completion fails or remains unknown according to the fixture outcome

### Requirement: Memory lifecycle evaluation
The evaluator SHALL cover extraction, retrieval, current and historical facts, corrections, forgetting and restart with source-bound evidence.

#### Scenario: Forgotten fact in an old checkpoint
- **WHEN** a retired fact is present in a checkpoint resumed after forgetting
- **THEN** the evaluator detects any new delivery or use that violates the current visibility contract

### Requirement: Reproducible comparison
Reports MUST preserve profile and dataset revisions, all planned outcomes, repeated-trial dependence and uncertainty; insufficient evidence is not a pass.

#### Scenario: Small comparison sample
- **WHEN** the non-inferiority interval crosses its preregistered margin
- **THEN** the comparison remains inconclusive or failed rather than passing by point estimate

### Requirement: Executable comparison baseline
A non-inferiority claim MUST use a source-pinned executable baseline, common synthetic-world adapters and a preregistered comparable capability set. Historical aggregate scores or a newly chosen easier comparator MUST NOT substitute for the baseline.

#### Scenario: Baseline cannot be reproduced
- **WHEN** the pinned baseline cannot run the common fixture protocol
- **THEN** non-inferiority is reported as not run and its release gate remains unmet; absolute alpha results may still be reported separately
