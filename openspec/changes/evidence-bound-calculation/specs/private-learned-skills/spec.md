## ADDED Requirements

### Requirement: Exact arithmetic with explicit current source origins
The foreground user SHALL be able to calculate from one uniquely identified complete current saved source and fresh current-message parameters using actual linked receipts and server-derived numeric origins.

#### Scenario: Unique literal field and current scalars
- **WHEN** a current literal query yields exactly one complete untruncated source at its current revision, an exact user-quoted prefix occurs exactly once, and fresh scalar quotes resolve uniquely without overlap
- **THEN** the server derives the adjacent strict decimal, evaluates the explicit exact graph and records actual read and arithmetic provenance without model-supplied source values or byte offsets.

#### Scenario: A digit appears inside a different amount
- **WHEN** a scalar quote matches one complete current numeric token and also appears only as a substring of another amount
- **THEN** only the complete token supplies the scalar, preserving the existing sign, delimiter, overlap, source and receipt checks
- **AND** two complete matches remain ambiguous and are rejected

#### Scenario: Ambiguous, stale or unsupported evidence
- **WHEN** source identity or field is ambiguous, evidence is stale or incomplete, a quote is absent/repeated/overlapping, a receipt belongs elsewhere, a decimal is unsupported, or a numeric quote is a grouping/expression fragment
- **THEN** no successful bound calculation or learning proof is created and the system declines or requests the missing explicit input
- **AND** missing or ambiguous current evidence does not count as a learned arithmetic structure failure.

#### Scenario: Korean suffixes delimit complete current numbers
- **WHEN** a unique fresh numeric quote is followed by a supported Korean particle, copula ending or counter plus optional particle, ending at an ordinary separator
- **THEN** source-bound calculations SHALL accept that complete number without converting units
- **AND** embedded signed/decimal/grouped numbers, ambiguous bare scale suffixes, operator fragments, arbitrary suffix words and repeated or overlapping quotes SHALL remain invalid.

### Requirement: Structural mixed-tool learning and isolated fresh reuse
A mixed source/calculation procedure SHALL store only typed origin-slot kinds and arithmetic structure, require two distinct finalized source/parameter proofs with terminal adoption, and reuse only fresh current-input reads within existing budgets and authority.

#### Scenario: Two proofs and fresh C
- **WHEN** two independent finalized current-owner runs adopt actual linked source/field/number and exact calculation receipts
- **THEN** a fresh learned execution performs a unique new source search/read and the pinned arithmetic with current quoted parameters and actual child costs
- **AND** same-slot replay returns its committed result without duplicate IO, receipts or charge.

#### Scenario: Withdrawal or older profile
- **WHEN** a source, example, owner authority or used route is withdrawn/disabled, or a run has an older profile without the new generation3 capability
- **THEN** existing dependency/late-output fences apply, unavailable examples cannot qualify, and old runs are not upgraded by a newer binary.

#### Scenario: Share structure without private evidence
- **WHEN** the user explicitly requests a share package
- **THEN** the package is rebuilt from plan structure with repository-owned synthetic source preconditions outside the current example request, unique noncolliding synthetic current quotes and current parameter instructions
- **AND** private anchors, values, identifiers, traces and eligibility proofs are not reconstruction inputs or transferred authority.
