## Why

P9 needs a concrete offline transfer path without touching legacy operating state
or treating copied source files as migration parity. Unsupported authority and
withdrawn/corrected private records must not reappear as usable current data.

## What Changes

- Add an explicit owner-selected normalized ordered JSON transfer contract and Rust
  CLI importing supported source, memory and document history into a new private
  SQLite staging root through normal Store operations.
- Preserve imported identity/version/evidence mappings and account every rejected
  event; quarantine failed corrections by withdrawing prior staged content.
- Enroll one fresh local account, admit no old authority, retain private sibling
  reports and ordinary SQLite report artifacts, and publish only verified new roots.

## Capabilities

### New Capabilities
- `private-import`: Bounded explicit private transfer staging and reconciliation.

## Impact

P9 and the private-import contract in docs/migration.md. No schema, dependency,
production source access, existing service changes, automatic legacy exporter,
legacy DB driver or cutover is added.
