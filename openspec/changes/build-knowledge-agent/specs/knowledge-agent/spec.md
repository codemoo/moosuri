## ADDED Requirements

### Requirement: Tool capability is proven and pinned
The system SHALL enable function calls only after a fresh two-round challenge for
the exact verified provider revision, model and effort, under current admin authority.

#### Scenario: False or stale capability
- **WHEN** a model emits a wrong function, wrong challenge, failed round or stale provider binding
- **THEN** capability is not recorded and existing runs retain their prior pin

### Requirement: Completed calls execute through durable bounded steps
The system SHALL commit validated model continuations and typed calls before executing
local reads, with opaque leases and budgets spanning all attempts.

#### Scenario: Retry reaches cumulative limit
- **WHEN** retries consume six model dispatches or twelve local reads
- **THEN** additional work is blocked with a budget outcome and prior receipts remain

#### Scenario: Invalid arguments are repairable
- **WHEN** a completed model call has semantically invalid or oversized typed arguments
- **THEN** a bounded error result is persisted and the model can repair within remaining limits

#### Scenario: Restart after ambiguous dispatch
- **WHEN** a process restarts with an admitted call whose outcome is unknown
- **THEN** no provider request is automatically replayed and explicit retry clears old private continuation state

### Requirement: Retrieved evidence remains account scoped and revocable
The system SHALL record only actually returned evidence revisions and inherit dependencies
from selected successful context, while enforcing current authority on every step/read/output.

#### Scenario: Source withdrawal after a dependent answer
- **WHEN** a source is deleted or a dependent memory is corrected or forgotten
- **THEN** affected derived output and private tool state are purged, late output is fenced, and user text and content-free accounting survive

#### Scenario: Oversized or foreign read
- **WHEN** a read targets another account or exceeds bounded UTF-8/result constraints
- **THEN** no foreign data is returned and the tool produces a bounded error without inventing provenance

### Requirement: Browser history does not revive withdrawn evidence
The system SHALL advance the thread reset floor on invalidation and replace cached
history on every full snapshot, fencing older-page and action responses by snapshot epoch.

#### Scenario: Offline browser misses reset
- **WHEN** a browser reconnects after evidence deletion with a fresh page and cursor beyond the reset
- **THEN** older cached answers/evidence are discarded and a late retry acknowledgement cannot restore them

#### Scenario: Exact reference navigation
- **WHEN** a user opens a source reference from an answer
- **THEN** the UI requests the backend-provided object ID and revision in the current account scope
