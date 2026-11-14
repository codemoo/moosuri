## ADDED Requirements

### Requirement: Source observation range is separate from validity
Source search SHALL accept optional `observed_from` inclusive and `observed_until` exclusive Unix-second bounds over the current source revision's recorded observation timestamp. It SHALL apply assertion validity separately and reject unsupported memory/mixed query semantics, invalid timestamps and empty/reversed intervals.

#### Scenario: Same validity, different observation
- **WHEN** sources share assertion validity but have different recorded observation times
- **THEN** only source timestamps within the explicit interval are returned, including its start and excluding its end.

### Requirement: Filtered search preserves lifetime and continuation fences
Observation bounds SHALL be available through authenticated API, native source read tool and existing knowledge search UI. Continuation SHALL bind the exact owner, range, other query fields and knowledge generation. Omitted bounds SHALL preserve legacy behavior and serialization.

#### Scenario: Changed interval or withdrawn source
- **WHEN** a continuation is reused with a different interval, another owner or changed source generation
- **THEN** it is rejected and a fresh query excludes withdrawn or foreign sources.

### Requirement: Observation dates do not become fixed reusable authority
Date-bound source searches SHALL NOT become source-learning proofs or evidence-calculation bindings while those routes lack structural date parameters. Existing unfiltered route/proof behavior SHALL remain compatible.

#### Scenario: Two successful date-bound reads
- **WHEN** two independent successful source reads used observation bounds
- **THEN** they create no reusable source route, while unfiltered independent reads retain ordinary learning eligibility.
