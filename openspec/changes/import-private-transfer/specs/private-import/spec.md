## ADDED Requirements

### Requirement: Explicit private transfer staging
The offline importer MUST read only an explicitly selected bounded private normalized
bundle and publish only a newly created SQLite root. It MUST map one selected legacy
owner to a newly enrolled account without admitting old sessions, credentials,
providers, execution, schedules or effect authority. It MUST NOT access a legacy DB,
change original services or infer production migration parity.

#### Scenario: Existing destination
- **WHEN** a destination or report directory already exists
- **THEN** import rejects without replacing it or rewinding enrollment/data

### Requirement: Ordered history and withdrawal safety
Supported source/memory/document histories MUST preserve explicit identity/version
mapping and validated references through existing mutation rules. Retired objects
MUST NOT be resurrected, and a rejected correction MUST withdraw its already staged
prior current content before publication.

#### Scenario: Source correction invalidates derived records
- **WHEN** an ordered source update follows imported memory/document evidence
- **THEN** the normal Store invalidation/withdrawal rules apply before publication

#### Scenario: Correction references missing source
- **WHEN** an imported correction has an invalid reference
- **THEN** that event and later invalid dependents quarantine and its previous
  staged current content is withdrawn rather than exposed as successfully corrected

### Requirement: Every event accounted and original retained
Every event MUST have applied/quarantined outcome, hash/position and reason. Unknown
or unproved categories MUST NOT be silently dropped or enabled. The original private
bundle MUST remain unchanged; reports MUST identify it and preserve mapping evidence
inside SQLite backup scope plus separate private sibling reports. Report artifacts
MUST NOT be mislabeled immutable provider receipts or latestness proof.

#### Scenario: Unsupported old work with a credential field
- **WHEN** the selected bundle includes an old execution/credential event
- **THEN** no old authority or raw credential payload enters active tables or logs;
  quarantine records its hash/position/reason and retains the unchanged original input
