## ADDED Requirements

### Requirement: Explicit bounded owner authority
The system SHALL admit a mission only from an authenticated explicit owner request and SHALL pin
its original task, constraints, checks, provider, tools, account epoch and admission expiry.

#### Scenario: Durable grant outlives logout
- **WHEN** an owner explicitly chooses durable execution and logs out
- **THEN** the stored grant MAY continue only under the pinned eligible account/epoch and closed read tools
- **AND** no UserContext or session is fabricated

#### Scenario: Foreground authority is withdrawn
- **WHEN** a current-session request loses its actual session
- **THEN** no further step is dispatched under that session

### Requirement: Evidence establishes bounded completion
The system SHALL require actual current-run typed receipts for every machine check and SHALL leave
semantic review unmet. Failed/missing/wrong receipts and a model's declaration SHALL NOT establish achieved.

#### Scenario: Exact result and quote
- **WHEN** the candidate matches an exact decimal receipt and owned source revision/quote
- **THEN** those declared checks pass deterministically

#### Scenario: Missing or fabricated evidence
- **WHEN** a model names absent/failed/wrong-run evidence or only states success
- **THEN** achieved is refused and at most two repairs use the original remaining budget

### Requirement: Stable recovery and revocation
The system SHALL preserve budgets and acceptance through pause/resume/restart, SHALL fence stale
leases, and SHALL never replay an unknown committed model or external dispatch.

#### Scenario: Stable pause then resume
- **WHEN** an owner pauses after committed local results and resumes with current revision
- **THEN** completed local slots replay stored results and the original deadline/counters remain

#### Scenario: Repeated stable continuation at the chat attempt boundary
- **WHEN** a stable mission resumes or restarts at the shared chat attempt32 boundary
- **THEN** its carrier and mission are queued together with a fresh run lease on claim
- **AND** stale run/step mutation is rejected while exact late usage receipts remain valid, without changing dispatch limits

#### Scenario: Process loss after dispatch
- **WHEN** a committed model/public HTTP dispatch has no established outcome
- **THEN** the mission becomes unknown and requires a new explicit request

#### Scenario: Withdrawal, deletion or restore
- **WHEN** source/skill/provider authority is withdrawn, the owner deletes a mission, or an installation restores
- **THEN** derived results or unfinished grants are fenced according to provenance, tombstone and quarantine rules
- **AND** stale callbacks and idempotent controls cannot resurrect private withdrawn output

### Requirement: Local acknowledged recovery is independent of dispatch capacity
A stable latest completed final proposal SHALL remain eligible for local verification after restart at exact model/tool capacity, without resetting or expanding dispatch limits. Unknown/started dispatches, pending tool work, expiry, withdrawal and original authority fences SHALL still block recovery as applicable. Ordinary chat retry admission SHALL remain unchanged.

#### Scenario: Crash after the last acknowledged proposal
- **WHEN** a valid final proposal with committed actual receipts is durably saved at model6or tool12and the process restarts before verification
- **THEN** the mission completes local verification without another model/tool/external dispatch or increased usage accounting

### Requirement: Owner control retries preserve current intent
An uncertain control SHALL reuse its UUID/payload only for retrying the same mission and exact action. A different card/action/instruction SHALL reconcile current revision and use a fresh operation rather than silently resending the earlier control. A known rejection SHALL retire that operation.

#### Scenario: Resume acknowledgement is lost before Cancel
- **WHEN** Resume commits but its acknowledgement is lost, polling observes a newer active revision and the owner clicks Cancel
- **THEN** the client sends a fresh Cancel for the observed current revision and never replays the old Resume as that cancellation

#### Scenario: Delayed Resume wins a cancellation CAS race
- **WHEN** the prior ambiguous Resume commits after a current-state read and causes the newly requested Cancel to conflict
- **THEN** the client reconciles again and retries that Cancel with a fresh current revision/UUID, without another Resume or approval prompt

### Requirement: Freeform conditions remain subject to owner review
The system SHALL NOT infer that typed calculation/source/output checks establish arbitrary freeform constraints. Nonempty prose constraints SHALL require explicit or implicit owner review without modifying the original request. Passing machine checks SHALL yield a reviewable partial result without extra model repair calls; failed machine checks SHALL retain their bounded repair behavior.

#### Scenario: Exact value with additional delivery conditions
- **WHEN** an API caller supplies a decimal check and nonempty delivery conditions without user_review
- **THEN** a matching decimal receipt leaves the result partial with an additional-conditions review report
- **AND** restart verification applies the same rule and does not spend another model call

#### Scenario: Exact value with no additional conditions
- **WHEN** a request has only a decimal check and no prose conditions
- **THEN** the actual matching receipt and output can establish that selected check
