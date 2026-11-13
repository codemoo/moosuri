## ADDED Requirements

### Requirement: Explicit immutable owner templates

The system SHALL admit a recurring read-only mission only through a current owner-authenticated save. It SHALL pin owner epoch, schedule revision, provider/model/effort and admitted read-only profile, and SHALL NOT manufacture a session or substitute a current default during polling.

#### Scenario: Generic reminder authority cannot become mission authority

- **WHEN** a reminder API/chat control targets a recurring template ID
- **THEN** it SHALL fail without mutating the template and generic reminder claim/publication SHALL exclude it.

#### Scenario: Saved binding becomes unavailable

- **WHEN** the pinned provider or owner authority is withdrawn
- **THEN** no new model dispatch SHALL use a replacement binding and current results SHALL retain truthful withdrawal/unknown state.

### Requirement: Bounded occurrence-bound admission

The system SHALL reuse Once/Interval/Daily/Weekly IANA time semantics, gap/fold policy and latest-only catchup. It SHALL atomically create at most one actual mission per occurrence, preserve existing owner/global capacity and persistent owner fairness, and make zero idle model calls.

#### Scenario: Commit fails or acknowledgement is lost

- **WHEN** polling is interrupted or repeated around child insertion
- **THEN** occurrence and mission SHALL both roll back or refer to the same committed mission.

#### Scenario: Work was missed while offline

- **WHEN** the latest eligible occurrence becomes due after downtime
- **THEN** it SHALL retain scheduled UTC identity and a deadline no later than due+24hours or validity end, and expired one-shots SHALL NOT receive a fresh execution window.

### Requirement: Parent controls fence unfinished children

The system SHALL fence child execution and uncommitted results on parent pause/edit/revoke, account epoch loss and restore quarantine while preserving actual usage and immutable committed receipts. Resume SHALL authorize only future occurrences.

#### Scenario: A dispatched child loses authority

- **WHEN** the owner pauses its parent after dispatch
- **THEN** the child SHALL remain unknown/cancelled without automatic replay and its late result SHALL NOT commit.

#### Scenario: A control acknowledgement is retried after new admission

- **WHEN** an already committed resume is retried
- **THEN** it SHALL NOT cancel a newly admitted child under the current revision.

### Requirement: Private observable results and exact equality

The system SHALL retain actual mission results and occurrence history. Immutable local notices SHALL contain no task/output/source evidence, only fixed status and a current-owner mission link. Changes-only SHALL suppress only exact checked decimal/source-backed achieved outputs under the same authority revision/epoch; unsupported evidence, partial/review, failure, unknown and invalid baselines SHALL notify when publication authority remains valid.

#### Scenario: Dispatch identity differs but actual checked result is identical

- **WHEN** two eligible successful occurrences have exact equal body and normalized decimal/source receipt values
- **THEN** the later notice MAY be suppressed while both actual mission records remain visible.

#### Scenario: Evidence is revoked after notice publication

- **WHEN** source withdrawal quarantines a prior result
- **THEN** the notice SHALL reveal no copied evidence and opening mission history SHALL NOT revive the withdrawn body or use it as an equality baseline.
