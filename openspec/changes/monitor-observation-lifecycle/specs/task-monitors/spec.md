## ADDED Requirements

### Requirement: Observation notices follow the current child result lifetime

A monitor observation MUST be associated with its actual child and remain unresolved
and usable before publication and at final inbox/push authorization. A quarantined,
deleted, withdrawn or execution-invalidated child MUST NOT publish or deliver a stale
notice. Independent valid monitor permission MUST remain available for later source
changes. A current partial review MUST remain visible with its actual receipt.

#### Scenario: Source changes before observation publication

- **WHEN** an achieved child is invalidated by a newer version of its read source
- **THEN** pending publication is fenced without creating a stale result notice
- **AND** a later review of the new version can run once under the existing valid permission

#### Scenario: Source changes or child is deleted after publication

- **WHEN** the current child result is withdrawn after its notice or push claim was saved
- **THEN** inbox read, push dispatch and accepted-push display reject that stale notice
- **AND** the old source suppression provenance is not rewritten

#### Scenario: Partial source review remains current

- **WHEN** a bounded review settles partial with an actual current source receipt
- **THEN** its notice remains visible and linked to that child
- **AND** incomplete coverage is not presented as achieved

### Requirement: Invalid reuse does not consume a fresh monitoring proposal

Selecting an expired or otherwise invalid reusable match MUST retire that match
using normal policy eligibility and continue proposal selection for the newly
completed task. It MUST NOT inherit the invalid permission or silently consent.

#### Scenario: Previously reusable monitor permission expired

- **WHEN** a new completed task has the same scope as an expired reusable monitor
- **THEN** the old monitor becomes expired and the new task receives a pending proposal
- **AND** activation uses only a new ordinary explicit acceptance, retaining its own limits
