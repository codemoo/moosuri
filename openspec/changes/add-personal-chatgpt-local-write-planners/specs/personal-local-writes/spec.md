## ADDED Requirements

### Requirement: Existing personal foreground typed local writes

Exact tool-verified personal foreground runs SHALL reuse current-user-only memory and artifact planners and source-neutral Store intent, target and receipt boundaries without a new approval or capability protocol.

#### Scenario: Explicit owner memory request

- **WHEN** the current owner asks to remember, correct or forget a supported fact
- **THEN** isolated classification and any sealed target selection SHALL use exact plan model/effort and Store SHALL commit only its typed authorized mutation and fixed receipt confirmation.

#### Scenario: Read-only artifact profile already exists

- **WHEN** an admitted run has a planned read-only artifact profile
- **THEN** it SHALL retain that profile without acquiring a retroactive write grant.

### Requirement: Planner transport and durable authority

Every personal planner model dispatch SHALL resolve current access through existing RunLease/refresh CAS, preflight matching dialect and durably admit its step before inference.

#### Scenario: Token expires before target selection

- **WHEN** a personal command needs a second candidate-selector call after token expiry
- **THEN** the runtime SHALL renew access under existing single-flight semantics before admission and SHALL NOT replay ambiguous inference or rotating refresh.

#### Scenario: Candidate or authority changes while planning

- **WHEN** a candidate, evidence revision, owner session or connection changes before commit
- **THEN** current Store guards SHALL prevent stale mutation and late confirmation.

#### Scenario: Prepared local command is retried

- **WHEN** a valid sealed memory command is explicitly retried
- **THEN** it SHALL resume before new model client construction or reclassification and SHALL NOT repeat its mutation after receipt commit.

### Requirement: Supported artifact write provenance

Artifact-write stages for shared and personal providers SHALL advertise only supported saved-evidence tools, while retaining Store provenance and singleton receipt guards.

#### Scenario: Model emits an undeclared external call

- **WHEN** a write-stage model emits mail, web, browser or Calendar function calls omitted from its definitions
- **THEN** existing allowed-name decoding SHALL reject the call before executable release without claiming that omitted definitions alone block all network paths.

#### Scenario: Derived artifact is written

- **WHEN** a valid typed artifact grant reads saved evidence and writes its pinned target
- **THEN** mutation, receipt and truthful confirmation SHALL commit together and no subsequent model inference SHALL claim success.

### Requirement: Separate capabilities remain staged

Personal local writes SHALL NOT enable Calendar mutations, mission delegation, background execution or media attachments.

#### Scenario: Local write capability is available

- **WHEN** a personal foreground run acquires a valid local write grant
- **THEN** existing Calendar/background/media fences and mail final-preview approval SHALL remain in force.
