## ADDED Requirements

### Requirement: Current candidate coverage

Semantic memory target selection SHALL apply its completeness limit to current,
unexpired effective assertions and SHALL NOT revive superseded assertions.

#### Scenario: Expired stored population exceeds the candidate budget

- **WHEN** more than64 expired stored assertions coexist with one current fact
- **THEN** a correction of the current fact SHALL still be eligible for isolated
  target selection and SHALL change only the selected current fact.

#### Scenario: Current population exceeds the candidate budget

- **WHEN** more than64 current eligible facts exist
- **THEN** selection SHALL clarify instead of choosing from a partial list.

### Requirement: Original-message date reference

Ordinary model context SHALL include a trusted UTC and local date/time reference
for the original user message, preserving it across retries and instruction profiles.

#### Scenario: Local midnight or DST differs from UTC

- **WHEN** an original request includes a valid timezone
- **THEN** context SHALL use that timezone's offset at the original timestamp,
  including the correct local date and both sides of a DST fold.

#### Scenario: Legacy request has no timezone

- **WHEN** an original request has no timezone
- **THEN** context SHALL explicitly label UTC fallback without inventing a local
  timezone or granting mutation authority.

#### Scenario: Retry after the original date

- **WHEN** a failed request is retried later
- **THEN** context SHALL preserve its original reference rather than changing the
  interpretation of the user's original relative-date instruction.

### Requirement: Bounded read-only memory routing

Recognized complete read-only memory lookups SHALL use ordinary read tools without
an additional mutation-classifier round; ambiguous or mixed mutation remains gated.

#### Scenario: Explicit bounded lookup

- **WHEN** the current user asks “내 회의 전 준비 시간이 얼마나 되는지 기억에서 찾아 알려줘”
  or “내 회의 전 준비 시간을 기억하고 있어? 없으면 모른다고 해 줘”
- **THEN** the memory mutation classifier SHALL be skipped and no write authority
  SHALL be created by that routing decision.

#### Scenario: Quoted or mixed mutation

- **WHEN** lookup text includes a correction, save, deletion or quoted command
- **THEN** it SHALL retain the existing isolated mutation-classifier route.
