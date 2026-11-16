## ADDED Requirements

### Requirement: Original task conversation follows current instruction eligibility
For existing bounded missions (R23, F04/F27), lifecycle writers
MUST publish the current instruction target to each owned original `create_now`
conversation when that target changes. Outstanding received guidance MUST NOT be
a prerequisite. Publication MUST preserve the task/carrier identity, authority,
execution budget and original admission outcome.

#### Scenario: Task completes without outstanding guidance
- **WHEN** a chat-origin mission completes with no guidance or only applied guidance
- **THEN** a subsequent durable event in the original conversation disables its instruction target
- **AND** the next ordinary message can be admitted without refreshing that conversation

#### Scenario: Original task waits and resumes
- **WHEN** an admitted task waits for bounded input and resumes after its answer
- **THEN** original-conversation events disable then restore instruction eligibility
- **AND** the restored target names the same carrier and current attempt

#### Scenario: Capacity or cancellation ends live eligibility
- **WHEN** the current mission reaches its model capacity or is cancelled
- **THEN** its original conversation receives a disabled target
- **AND** unchanged polling and repeated accepted controls add no duplicate eligibility events

#### Scenario: Retention and removal
- **WHEN** the prior origin projection has been pruned and the linked task is deleted or quarantined
- **THEN** the affected lifecycle mutation still emits a current target-removal event
- **AND** no deleted target or withdrawn task data is restored

### Requirement: Scoped writer-owned event publication
Only the affected carrier's tenant/account-matching original conversations may
receive target updates. Existing writer transactions MUST own publication;
SSE event reads MUST NOT mutate state or scan arbitrary account history.

#### Scenario: Unchanged target
- **WHEN** an existing lifecycle hook observes the same target already represented in its latest durable state event
- **THEN** it writes no target event and advances no origin cursor
