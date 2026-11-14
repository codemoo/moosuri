## ADDED Requirements

### Requirement: Bounded versioned Korean aliases are optional
Saved-knowledge search SHALL default to unchanged literal matching. Optional `korean_aliases_v1` SHALL expand only the three immutable public families, at most three complete normalized query variants, when the full normalized query or exactly one raw whitespace word exactly matches a family.

#### Scenario: Phrase and hard-negative comparison
- **WHEN** a caller searches `분실 휴대폰 보험` in alias mode
- **THEN** complete normalized phrases with the three phone terms match, while extra intervening words do not.
- **AND** multiple matching words, unknown/inflected/punctuated family words fall back literal.

### Requirement: Union precedes pagination and retains authority
Alias result union SHALL deduplicate current id/revision before stable pagination and retain tenant, identity, scope, validity, observation-time and deletion fences. Mode/version SHALL bind to cursors. Absent/null/literal mode SHALL preserve legacy query serialization.

#### Scenario: Overlap and changed continuation
- **WHEN** one source matches multiple aliases and more than20 eligible sources exist
- **THEN**20+1 pagination returns each once, and changing mode/text/date or owner invalidates continuation.

### Requirement: New matching scope does not silently qualify old learning
Alias-mode reads SHALL NOT create source-learning proofs or evidence-calculation bindings while those routes lack a structural mode parameter. Legacy literal source routes and generation3 SHALL remain compatible.

#### Scenario: Two alias successes then literal successes
- **WHEN** two independent alias reads complete
- **THEN** no reusable source route is created, while two ordinary independent literal reads retain existing qualification.
