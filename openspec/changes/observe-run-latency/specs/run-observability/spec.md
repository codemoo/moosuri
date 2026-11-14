## ADDED Requirements

### Requirement: Content-free observed timing
The server SHALL collect measurable queue/context/dispatch/model/tool/final-body/
terminal observations using existing run and step identities. Model, effort,
authority, counters and budgets SHALL remain unchanged. Exported timing DTOs SHALL
contain only numbers, booleans and fixed outcome keys without content/private IDs.

#### Scenario: Commentary and authoritative final phase
- **WHEN** commentary precedes an eventual explicit final body or phase resolution
- **THEN** only surviving nonrefusal final body qualifies for first-final timing
- **AND** absent phase, tool-only, partial or failed output remains a missing sample

### Requirement: Honest incomplete and retry measurements
Failed/cancelled/unknown runs SHALL remain in outcome/sample denominators. Missing
span ends, historical records and cross-clock/restart intervals SHALL NOT be zero
or fabricated elapsed values. Explicit retry SHALL preserve original attempt IDs
and previous observations without changing cumulative execution budgets.

#### Scenario: Interrupted stream and reopen
- **WHEN** a stream truncates or is cancelled, or an active installation reopens
- **THEN** observed usage/outcomes remain factual while unobserved durations stay null
- **AND** no timing operation dispatches another model or resurrects output

### Requirement: Bounded private usage reporting
Existing owner/admin usage surfaces SHALL present bounded retained-history numeric
aggregates with fixed outcome and sample/missing denominators. Absent percentile
samples SHALL be null; server observations SHALL NOT imply browser/render latency.

#### Scenario: Another account and legacy absence
- **WHEN** an owner reads diagnostics beside another account and old unmeasured runs
- **THEN** only their own timings appear, absent values are clearly missing, and admin
  reporting contains only the existing authorized account aggregate scope
