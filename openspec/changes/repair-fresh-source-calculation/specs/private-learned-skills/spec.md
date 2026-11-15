## ADDED Requirements

### Requirement: Fresh source arithmetic with server-assigned receipts
A newly admitted run SHALL be able to request one typed current-source calculation
whose search, complete read and arithmetic retain actual same-run child receipts.
Old persisted profiles SHALL NOT acquire this capability implicitly. Source
uniqueness, current owner/revision, numeric bounds and existing budgets SHALL remain enforced.

#### Scenario: Repeated literal source selector
- **WHEN** a current request repeats the same literal query in its requested output title
- **THEN** the server records its first exact span and independently requires one current source and one source field
- **AND** duplicate complete scalar tokens or overlapping parameter ranges remain invalid

#### Scenario: Recoverable selection or field failure
- **WHEN** a fresh calculation has invalid current parameters, nonunique sources or an invalid source field
- **THEN** it reports a bounded corrective category, charges actual work and produces no successful arithmetic or learned proof

#### Scenario: Actual qualification and replay
- **WHEN** two independent owner-adopted reports use the fresh composite route
- **THEN** qualification revalidates real child reads/arithmetic and adopted source facts before enabling the structural recipe
- **AND** later reuse reads fresh evidence without copying proof values or granting any send approval

#### Scenario: Acknowledged slot replay
- **WHEN** the same completed tool slot is replayed
- **THEN** the committed result is returned without additional reads, child rows or charges
