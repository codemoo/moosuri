## Purpose

Select useful tests based on user outcomes and material failure modes rather than legacy implementation structure or test counts, and retain honest evidence for failures and unexecuted checks.

## Requirements

### Requirement: Valuable new tests
Every new test SHALL identify a consequential defect and an independent observable oracle; legacy tests MUST NOT be ported solely because they exist.

#### Scenario: Duplicate internal assertion
- **WHEN** a proposed test repeats an already covered internal helper shape
- **THEN** it is omitted or consolidated unless a distinct failure mode is demonstrated

### Requirement: Separate outcomes
Evaluation reports SHALL distinguish actual mission completion, appropriate blocked handling, partial success, infrastructure failure and not-run cases.

#### Scenario: Missing capability
- **WHEN** an agent correctly explains an unavailable connector
- **THEN** the result may pass handling criteria but is not counted as an executed mission success
