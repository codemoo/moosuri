## Why

The plan named backup/restore and scheduler recovery but omitted required file bytes and deterministic timing rules. A database-only restore could lose user attachments, while unspecified DST and misfire behavior could run the wrong occurrences.

## What Changes

- Back up referenced immutable files with a snapshot manifest, durable pins and complete encrypted archives; validate restore into an empty data directory.
- Define offline operator backup identities, retained epochs and independent journal verification inputs so encrypted recovery does not depend on the lost host or restored vault.
- Define wall-clock/interval, DST, downtime, pause/resume and edit semantics and serialize dispatch commitment with current revisions.
- Preserve existing user features and use the existing SQLite writer and journals rather than adding services.

## Capabilities

### New Capabilities
- `runtime-recovery`: Complete data restoration and deterministic recovery of scheduled work.

### Modified Capabilities

None.

## Impact

R19/R20/R22/R23, F10/F11/F14/F15/F29/F30/F33/F36/F52/F53/F61/F64. P1 defines storage/commit contracts, P5 scheduler semantics, P7 files and P9 clean-host restore. See [architecture](../../../docs/architecture.md), [migration](../../../docs/migration.md) and OPS03/AUTO01/AUTO02. This is a planning correction; no runtime has been implemented.
