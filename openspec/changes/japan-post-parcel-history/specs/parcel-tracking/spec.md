## ADDED Requirements

### Requirement: Separate official visible-history lookup
The system SHALL expose parcel_history only with the current exact parcel recipe
pin and enabled current public reads. It SHALL issue one fixed direct Japan Post
GET using a literal current-user or same-thread typed-receipt number. It SHALL
retain the existing owner/session/budget/nonce/cancel/unknown-outcome boundaries.
No summary preflight, retry, arbitrary destination or external write is added.

#### Scenario: Exact visible history
- **WHEN** one detailed item identity matches the requested number and all supported paired history rows parse within bounds
- **THEN** the result SHALL include provider-ordered literal date/time, status, details, office, region and postcode with actual URL/time
- **AND** coverage SHALL mean official visible history on that page, not globally complete worldwide records.

#### Scenario: Unsupported or incomplete history
- **WHEN** only a summary, another number, duplicate tables, unknown/reordered header labels, extra th/td, broken rowspan/colspan pairing, malformed rows or an unsupported event count is returned
- **THEN** the result SHALL contain no history events and SHALL NOT claim full-history success.
- **WHEN** the exact established not-found shape is returned
- **THEN** absence SHALL be reported separately from provider_changed.
- **WHEN** size limits are exceeded
- **THEN** failure SHALL be explicit instead of silently dropping events.

#### Scenario: Missing year or offset
- **WHEN** the provider date/time omits year or UTC offset
- **THEN** that omission SHALL be preserved without invented year/timezone/absolute timestamp or date sorting.

#### Scenario: Old pinned run
- **WHEN** an old1.0.0 or stale-digest profile submits a crafted typed history request
- **THEN** Store SHALL refuse dispatch even if the new wire variant is decodable.

### Requirement: Preserve latest-summary learning
Existing parcel_track and japan-post-latest-v1 SHALL keep their summary-only
contract. History results SHALL NOT count as summary-only learning proofs or grant
mission/subscription/action authority. Follow-ups MAY reuse history identity but
SHALL perform new IO for a current status or history.

#### Scenario: Learned summary remains a summary
- **WHEN** a private japan-post-latest-v1 route executes with a fresh supported number
- **THEN** it retains its latest-summary read and proof contract
- **AND** a separate history result does not become a summary-learning proof
