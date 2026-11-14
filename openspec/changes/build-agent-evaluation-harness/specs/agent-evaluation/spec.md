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

### Requirement: Bounded public regression execution
The initial evaluator SHALL execute only fresh synthetic worlds through authenticated
HTTP, Runtime and SQLite with a compiled loopback-only provider profile. It MUST NOT
accept a private database, live endpoint or credential.

#### Scenario: Skill text is read without task execution
- **WHEN** the scripted provider reads a skill and claims calculation completed without source-read and calculation receipts
- **THEN** independent grading rejects completion, even if prose contains the expected number

#### Scenario: Foreign candidate is never cited
- **WHEN** a search result contains another owner's object before final-answer selection
- **THEN** the evaluator rejects the observation even if final dependencies contain only owned objects

#### Scenario: Receipt is reused for another dispatched call
- **WHEN** one saved tool slot is duplicated in place of another issued call's result
- **THEN** the bijective dispatch-step/call/argument check rejects completion

#### Scenario: Durable mission completes and the store reopens
- **WHEN** an explicit synthetic mission plans, reads saved evidence, calculates and verifies its result
- **THEN** the evaluator independently reads its achieved state, plan, candidate and linked receipts and confirms their persistence without new dispatch after reopening

### Requirement: Apparatus and handling results remain distinct
Reports SHALL preserve every planned case, separate task/actual-mission eligibility
from failure-handling eligibility and keep negative controls outside primary success
counts. Source identity MUST describe actual build inputs, including dirty differences.

#### Scenario: Setup fails or execution times out
- **WHEN** a planned world cannot be prepared or finish its bounded workflow
- **THEN** its row remains not-run or failed respectively, with available elapsed time, dispatches and usage; unavailable stages/cost remain null

#### Scenario: Finalization is interrupted
- **WHEN** a success claim and tool results exist without the persisted completed no-call final step
- **THEN** completion fails and the negative-control result cannot increase primary success
