## ADDED Requirements

### Requirement: Mission execution starts without a narrative-only inference
The system SHALL provide truthful bounded task-derived plan labels and SHALL use the first model round of a fresh mission acceptance for ordinary admitted execution or a missing-input question. It SHALL preserve the separate authenticated current-owner intent planner, full accepted task/constraints/checks, selected model/effort and original model/tool/time limits.

#### Scenario: Requested source calculation and private report
- **WHEN** an admitted mission reads a current source, computes exact arithmetic and saves the explicitly requested private report
- **THEN** it produces the same actual source/calculation/document receipts with one fewer model dispatch than a separate narrative-planning round
- **AND** the plan labels neither grant an effect nor claim success.

### Requirement: First-round admission remains recoverable and fenced
The system SHALL admit current report/action contracts through the existing guarded completed-step and acceptance-revision boundary before executing first-round tools. It SHALL retain acknowledged non-planning proposals when saving local plan labels and SHALL consume them after restart without another inference or repeated successful tools.

#### Scenario: Restart around local plan saving
- **WHEN** the first execution round was acknowledged and the process stops before or after plan saving
- **THEN** recovery admits the same accepted contracts, executes only unfinished original slots and verifies the same result without another planning request or duplicate document.

#### Scenario: First final candidate was already acknowledged
- **WHEN** local plan saving is followed by restart with an acknowledged final candidate and no unfinished tools
- **THEN** local verification consumes that candidate without another model request.

#### Scenario: Revised acceptance and legacy acknowledgement
- **WHEN** an owner revises a stable paused task or a prior release's narrative plan was already acknowledged
- **THEN** the active acceptance revision, original spent budget and legacy acknowledged recovery remain valid while superseded receipts do not satisfy current checks.

#### Scenario: Revoked or uncertain execution
- **WHEN** account/provider/source authority is withdrawn, a run is paused/cancelled or an actual dispatch outcome is unknown
- **THEN** display labels and acknowledged history do not revive authority, reset limits or authorize a repeated external action.
