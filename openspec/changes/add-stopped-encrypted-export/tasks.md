## 1. Implementation
- [x] 1.1 Persist installation binding and public key epoch history through migration0013.
- [x] 1.2 Add stopped existing-installation setup and independent-identity decrypt proof.
- [x] 1.3 Add verified SQLite Backup snapshot and bounded fixed tar/age export publication.
- [x] 1.4 Generate a separate owner-private age identity with native Rust keygen, without opening a database or printing secret material.

## 2. Verification
- [x] 2.1 Verify WAL, lock, no-mutation rejection, metadata/epoch retention, hashes and failure boundaries.
- [x] 2.2 Verify reference Go age interoperability, Rust1.89 lock/MSRV/advisories and local checks.
- [x] 2.3 Independently review storage/crypto/CLI and final integrated Linux artifact, retaining initial fixture/transfer failures and affected-suite rechecks.

This change is export only. It does not complete historical restore, independent
latest-journal publication, authoritative media/pins or recovery accounting gates.

Evidence: [local export boundary record](../../../docs/reviews/2026-11-13-stopped-encrypted-export.md).
