## ADDED Requirements

### Requirement: Explicit conversational facts may carry a bounded literal date scope
The system SHALL derive remember/correct validity only from supported leading
absolute date directives in the current authenticated message and its immutable
original timezone. A date-only start SHALL include local midnight; an end date SHALL
include the entire local day by storing next local midnight as an exclusive bound.

#### Scenario: Future remember and historical expiry
- **WHEN** a valid leading directive identifies a future start or past inclusive end
- **THEN** the saved validity controls real current/as-of retrieval and the confirmation shows inclusive dates derived from actual saved bounds in the original timezone

#### Scenario: Scheduled correction begins and expires
- **WHEN** a dated correction is applied to a unique current target
- **THEN** the prior effective revision remains before the start, the new revision applies at the start, and expiry does not revive the old assertion

### Requirement: The Store independently rejects unsupported temporal scope
The Store SHALL examine the complete message independently of model quotes and
clarify without mutation for recognized unsupported or unresolvable temporal scope.
Ambiguous words SHALL require explicit temporal context; ordinary date facts SHALL
remain facts rather than becoming validity bounds.

#### Scenario: The classifier strips temporal text from its fact quote
- **WHEN** the current message contains recognized unsupported scope but its model fact quote omits that text
- **THEN** the Store produces editor guidance before target selection and writes no undated memory

#### Scenario: An ordinary assertion contains ambiguous boundary vocabulary
- **WHEN** an ordinary fact contains 기준/적용 or “through” without temporal context
- **THEN** it remains storable undated, with a supported leading date directive, and through legacy prepared undated recovery

#### Scenario: A date-only request has no original timezone or a skipped midnight
- **WHEN** the original message lacks a timezone or midnight is ambiguous/nonexistent there
- **THEN** the command clarifies without using the host timezone or current time

#### Scenario: A dated forget request is presented
- **WHEN** a forget request includes an effective boundary
- **THEN** the system clarifies without deleting now or scheduling a future deletion

### Requirement: Sealed temporal scope remains stable through recovery
The system SHALL seal server-derived bounds in existing plan storage, validate the
seal against immutable request context on load, and preserve existing receipt,
head/effective CAS, invalidation and run-wide budget behavior.

#### Scenario: Retry or reopen follows a mutable continuation timezone change
- **WHEN** a sealed dated command resumes after restart or explicit retry
- **THEN** it retains original bounds, commits once, and needs no replacement classifier call

#### Scenario: An old or malformed stored plan is loaded
- **WHEN** a legacy undated intent or malformed/temporal legacy intent is recovered
- **THEN** valid undated commands remain compatible and invalid temporal scope never falls back to null bounds
