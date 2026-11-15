## ADDED Requirements

### Requirement: Exact numeric adoption accepts equivalent bounded fixed-point formatting

For F26/P4 private exact-operation proofs, the system SHALL compare complete
answer and receipt numbers as exact fixed-point strings bounded to128bytes each,
normalizing both without floating-point approximation or tolerance. Primitive,
workflow and evidence checks SHALL share this comparison while preserving
existing receipt, authority and canonical-input rules.

#### Scenario: Two formatted answers promote one exact route

- **WHEN** two distinct finalized same-owner sum receipts contain exact results `140` and `38.6`
- **AND** the corresponding final answers adopt `**$140.00달러**` and `38.60달러입니다`
- **THEN** the route becomes usable from these two actual receipts
- **AND** a commuted/canonically equivalent input replay with another answer formatting remains only the original proof

#### Scenario: Equivalent evidence literal and generated precision

- **WHEN** a supported evidence source literal is `00023.2000` and its adopted answer number is `23.2`
- **THEN** their numeric adoption comparison succeeds subject to the existing evidence/title/prefix/receipt checks
- **AND** exact generated workflow precision is compared without truncation to the six-place external-input parser

#### Scenario: Optional formatting preserves exact value

- **WHEN** only positive signs, leading integer zeros or trailing fractional zeros differ
- **THEN** adoption SHALL preserve mathematically identical values, including signed zero
- **AND** nonzero signs and every significant digit SHALL still match

### Requirement: Numeric adoption requires complete supported tokens

The system SHALL accept ordinary currency prefixes, Korean units/sentence suffixes,
Markdown emphasis/code and one terminal sentence period. It SHALL reject numeric
fragments inside versions, malformed signed/dotted or grouped numbers, exponents,
percentages, expressions and ASCII identifiers. Unsupported surrounding syntax
SHALL fail conservatively.

#### Scenario: Changed signs and numeric fragments are rejected

- **WHEN** the exact receipt is `23.2` and the only answer number is `-23.20`, `23.200001`, `23.2.3`, `23.20%`, `1,23.20`, `23.20,000`, `23.20e0` or `1/23.20`
- **THEN** the answer establishes no numeric adoption proof
- **AND** Markdown emphasis around an unsupported fragment does not make it a complete supported number
