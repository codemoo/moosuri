## ADDED Requirements

### Requirement: Explicit complete relation snapshot
The local adapter MUST require bounded schema23 relation JSONL with complete
inventory/trailer, exact migration checksums and an explicitly selected single owner.
It MUST NOT connect to old services, parse raw pg_dump or admit old authority.

#### Scenario: Truncated or mismatched relation export
- **WHEN** the trailer, inventory, row count, required relation or ledger mismatches
- **THEN** the complete import rejects before publication

### Requirement: Truthful current projection and suppression
Eligible joins MUST validate current source/support/identity/projection/reference
state. Legacy revision N MUST map to new1 with N as metadata, without fabricated
history. Suppression MUST withhold implicated source admission, or all sources when
scope cannot be proved. A report MUST NOT claim native identity suppression parity.

#### Scenario: Corrected owner value and suppressed alias
- **WHEN** a valid current owner claim has an old suppressed alias
- **THEN** the current value may import, implicated source admission is withheld,
  and the old value/history is never invented

### Requirement: Complete bounded reconciliation
Every original row MUST receive a hash/index/outcome, with unsupported payloads
remaining solely in unchanged input. Limits MUST reject whole inputs without
truncation. Reports MUST publish before fresh data and remain in backup-scoped
bounded documents plus private sibling files.

#### Scenario: Unsupported credential payload
- **WHEN** an unknown relation row contains a secret
- **THEN** its hash/count/outcome is retained without copying raw payload to active
  data, reconciliation or diagnostics
