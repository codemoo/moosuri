## ADDED Requirements

### Requirement: Offline artifact and source-lock byte binding
The packager MUST bind exact binary size/hash and target/public metadata to declared
Git revision/dirty state, queried compiler metadata and exact generated inventory
bytes. It MUST NOT claim authenticated builder/build execution, minimal binary
dependency composition, license clearance or vulnerability verification.

#### Scenario: Existing signed bundle
- **WHEN** packaging encounters a signature, provenance reference or reserved sidecar
- **THEN** it refuses to overwrite or invalidate those existing artifacts

### Requirement: Authenticated bounded evidence admission
Evidence-bearing bundles MUST verify the pinned manifest signature before parsing
references and validate exact bounded binary/provenance/inventory bytes before
installation/update state mutation. Only fixed basenames may be read or staged.
Verified owned snapshots MUST prevent path replacement from substituting content.

#### Scenario: Tampered inventory or traversal reference
- **WHEN** inventory bytes mismatch the signed chain or a reference names another path
- **THEN** installation rejects before creating runtime state or backups/migration markers

### Requirement: Compatibility and retained verification
Existing format1 bundles without evidence MUST remain supported without a provenance
claim. Evidence MUST remain in immutable slots and participate in existing slot
verification and conservative uninstall. An old manager MUST be refreshed before
selecting a new evidence-bearing slot, including same-schema activation/recovery.

#### Scenario: Interrupted same-schema evidence activation
- **WHEN** recovery explicitly selects the new signed evidence-bearing slot
- **THEN** its evidence is reverified and manager refresh completes under the pending
  fence before pointer publication; no database rollback occurs
