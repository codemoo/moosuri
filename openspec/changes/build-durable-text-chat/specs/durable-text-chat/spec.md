## ADDED Requirements

### Requirement: Idempotent private submission
The server MUST bind each request ID to its tenant, thread and payload digest, save one user turn and return the same run on a duplicate submission.

#### Scenario: Submission acknowledgement is lost
- **WHEN** the client repeats the same request ID and text
- **THEN** no duplicate user message or model dispatch is admitted

### Requirement: Pinned execution and attempt fencing
Every attempt MUST retain its verified provider revision, model, effort and original account authority. Stale attempt results MUST NOT update a retried or cancelled run.

#### Scenario: Retry while a prior response is late
- **WHEN** an explicit retry starts a newer attempt
- **THEN** the prior attempt cannot append output or complete the new attempt

### Requirement: Crash-aware model dispatch
The store MUST record dispatch before network IO and MUST classify previously dispatched work as outcome_unknown after restart without automatic replay.

#### Scenario: Server exits after provider acceptance
- **WHEN** the process restarts without a committed terminal result
- **THEN** partial checkpoints remain and the model request is not automatically sent again

### Requirement: Independent bounded execution
The runtime MUST own model tasks independently from browser connections and bound admission, concurrency, context, output and time.

#### Scenario: Browser closes during an answer
- **WHEN** a browser disconnects without cancelling the run
- **THEN** authorized execution may finish and the answer is available after reconnect

### Requirement: Revocation and deletion fencing
Private reads, stream delivery and output writes MUST revalidate current authority. Thread deletion MUST remove private messages/events and prevent late output from recreating them.

#### Scenario: Thread is deleted while a model waits
- **WHEN** deletion commits before the pending output
- **THEN** no subsequent private output is saved or delivered for that thread

### Requirement: Durable incremental streaming
Events MUST have tenant-scoped monotonically increasing cursors, preserve assistant phase and reset clients when replay history is unavailable.

#### Scenario: Reconnecting cursor was pruned
- **WHEN** a client requests events older than the retained window
- **THEN** it reloads a canonical snapshot without duplicating text

### Requirement: Truthful usage observations
Each attempt MUST have one minimal call receipt, duplicate cumulative usage MUST NOT be summed and unknown usage MUST remain distinguishable from zero.

#### Scenario: Cancelled attempt reports final usage
- **WHEN** late accounting arrives through the original valid receipt capability
- **THEN** reported counters are recorded without publishing cancelled answer text or claiming a refund

### Requirement: Retry preserves the draft
Retry MUST operate on the saved run and MUST NOT replace the user's current composer text or create another user message.

#### Scenario: User writes a new draft then retries a failed answer
- **WHEN** retry is selected
- **THEN** the draft stays unchanged and the saved failed turn acquires a new attempt
