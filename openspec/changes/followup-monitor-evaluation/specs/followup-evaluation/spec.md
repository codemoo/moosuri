## ADDED Requirements

### Requirement: Grounded saved-source follow-up lifecycle

The public evaluator SHALL execute one independently gradable synthetic saved
return-status lifecycle through actual authenticated HTTP, Runtime and SQLite.
Owner-visible inputs SHALL be separate from evaluator-only grading observations.

#### Scenario: Unchanged scope has no inference

- **WHEN** an owner accepts a real completed-read monitor proposal and replays the same acceptance
- **THEN** exactly one scoped policy/schedule SHALL exist and an unchanged due occurrence SHALL save a no-new-evidence skip with no child, model/tool dispatch or notification

#### Scenario: One changed revision

- **WHEN** HTTP ingestion changes the same saved source
- **THEN** exactly one child SHALL read the changed revision, cite its actual receipt, verify the saved-source claim and publish one local notice whose mission, schedule and occurrence identities resolve to that child and its persisted occurrence; a later unchanged occurrence SHALL duplicate neither child nor notice

#### Scenario: Stop survives reopen

- **WHEN** the owner stops the monitor, revokes the policy and replays stop
- **THEN** policy and schedule SHALL remain revoked, the exact saved notice SHALL remain dismissed/push-suppressed and hidden through the inbox after reopen, and a later tick SHALL admit no child or inference

### Requirement: Honest stage and scheduling evidence

The evaluator SHALL reuse existing stage accounting and retain setup/workflow
failures and not-run stages in the planned denominator. Synthetic due-time
advancement SHALL be explicit, monotonic and within original expiry, using public
Store methods rather than direct DB writes. Independent graders SHALL reject
duplicate children, widened scope, missing changed reads, visible stopped notices
and revived policies. Missing usage/timing SHALL NOT be filled from fixture values.


#### Scenario: Literal quote and notice linkage reject false positives

- **WHEN** a result preserves positive source words but adds a paid claim or denies approval, or the visible notice retains its ID while linking to another mission, schedule or occurrence
- **THEN** the independent evaluator SHALL reject it
- **AND** this explicit quote task SHALL compare complete independently authored expected quotes after whitespace normalization in source receipt, saved candidate and output; this SHALL NOT be claimed as a general semantic-summary oracle
