## ADDED Requirements

### Requirement: Explicit encrypted journal checkpoint
Export MUST bind actual healthy source UUID/head and complete journal bytes in a
distinct bounded age archive/receipt. It MUST NOT seal, initialize missing source
evidence, change sessions or export unlock secrets.

#### Scenario: Manual stopped export
- **WHEN** normal source writer admission is available
- **THEN** export preserves source admission and returns the exact receipt hash for
  a separately trusted operator handoff

### Requirement: Pinned exact-prefix forward recovery
Historical restore MUST require the later receipt hash, same source, non-regressing
sequence and exact snapshot journal prefix. Actual semantic replay MUST reproduce
every sequence/head and final journal byte digest before publication. Normal
service crash-tail admission MUST remain unchanged.

#### Scenario: Pinned impossible adoption
- **WHEN** a hash-valid selected transcript adopts an absent/invalid operation
- **THEN** recovery rejects without publishing a destination

### Requirement: Quarantined authority and idempotent coverage
Later deletion, key/header/nonce reservations and receipts MUST survive recovery.
Imported sessions and pending authority MUST be quarantined before fresh identity
publication. Exact rerun MUST preserve later work. Selected coverage MUST NOT be
called independently latest/off-host or zero loss.

#### Scenario: Different pin or plain restore on historical root
- **WHEN** different recovery evidence or mode is supplied for an occupied root
- **THEN** it rejects rather than rewinding data or repeating quarantine
