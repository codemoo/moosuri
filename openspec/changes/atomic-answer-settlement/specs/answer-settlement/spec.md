## ADDED Requirements

### Requirement: Atomic foreground answer disposition
A completed foreground model answer SHALL acknowledge its assigned guidance and
choose completion or continuation in the same guarded writer transaction.

#### Scenario: Completion write fails
- **WHEN** final completion fails after the guidance mutation
- **THEN** the entire disposition rolls back, including receipt state and events,
  without claiming that the guidance was applied or the run completed.

#### Scenario: A newer instruction arrived
- **WHEN** a completed answer has a newer unassigned instruction
- **THEN** its assigned guidance is acknowledged and its output becomes commentary
  atomically before another model round is permitted.

#### Scenario: Superseding output fails
- **WHEN** the previous answer cannot be demoted for newer guidance
- **THEN** the whole disposition fails and no follow-up inference is authorized.

#### Scenario: Current authority and completion evidence
- **WHEN** settlement uses a stale/cancelled step, an uncommitted response, pending
  tool work, a local memory decision or a mission requiring verification
- **THEN** foreground completion is rejected without acknowledging new guidance.

#### Scenario: Store pressure
- **WHEN** a local acknowledgment or disposition encounters temporary Store Busy
- **THEN** only that local write may retry within the existing limit, while unknown
  acknowledgments and model/tool dispatch are never replayed.
