# Stopped encrypted export — 2026-11-13

This slice adds local `backup keygen/setup/create`, public-only SQLite installation/key
history and a standard age-v1 encrypted tar export. It does not implement restore,
unattended backups, an independent latest checkpoint, media/pins or vault recovery.

## Implemented boundary

Setup/create require an existing current-schema initialized installation and the
existing exclusive writer lock. Neither calls runtime `Store::open`, migrations,
startup journal replay or provider workers. A private DB+WAL clone avoids original
SQLite sidecar mutation on rejected paths; SQLite Backup produces the snapshot.
The source private provider-key file is neither needed nor opened. Setup's
independent identity is used only for a small synthetic decrypt proof, then only
its public recipient and retained epoch are committed atomically in SQLite.

The archive has exactly four fixed regular tar entries inside age encryption:
manifest, snapshot DB and two snapshot-era journals. SHA256 sizes/hashes and
journal-prefix watermarks are checked. The authoritative-file inventory is
explicitly empty. Tar and age finish, file fsync and no-overwrite parent-directory
publication precede the success report. No private identity/path, provider key,
external configuration, browser profile or user-vault key is included.

## Local evidence

macOS arm64, Rust1.89, local SQLite and synthetic accounts/files:

- Seven default store boundary tests pass, plus one explicitly selected reference
  interoperability case. These cover current/retainedA/B epochs, decrypt proof,
  missing server key, manifest byte hashes and fixed entry names, wrong keys,
  malformed journal/config history, source-size/deadline/output-writer failure,
  no overwrite/links, unsupported/empty/accountless/missing-journal installation
  rejection with original file-set/hash preservation, and pending auth-journal
  intent preserved without replay.
- A child exits without Rust drops, leaving initialization/account state only in
  committed WAL. Setup and the export include that state; holding the installation
  writer lock rejects setup/create. A separate CLI subprocess also rejects a live
  writer. This is macOS synthetic process-exit evidence, not Linux kill acceptance.
- Two regular CLI boundary tests pass, including native key generation before
  installation, secret-free output, permissions, no overwrite and path rejection.
  A separate explicitly selected reference Go age/age-keygen flow verifies the
  native-generated identity's public recipient, runs actual CLI setup/create
  with the server key removed, and decrypts the resulting archive successfully.
- Go age1.3.1 decrypts an actual application archive; Rust age decrypts Go-produced
  synthetic ciphertext. Ciphertext tampering fails. The reference binaries are
  verification tools only and are not runtime application dependencies.
- Workspace/all-target clippy with warnings denied, formatting, plan/link checks
  and strict OpenSpec change validation are recorded with the final slice freeze.

The dependency pin is age0.11.5/default-features=false, tar0.4.46/default-features=false
and the existing rusqlite Backup feature. The lock adds61 packages without
upgrading/removing baseline packages; its highest declared new transitive MSRV is
1.88 and the actual build uses1.89. OSV review finds build-time maintenance notices
for basic-toml0.1.10/RUSTSEC-2026-0374 and proc-macro-error2 2.0.1/RUSTSEC-2026-0173
through localization proc macros. These remain disclosed; no zero-advisory or
complete cryptographic-security claim is made.

## Integrated acceptance

Migration0013 follows the published community0011 and memory0012 migrations;
CURRENT_SCHEMA_VERSION and the SQL version are13. Independent storage/crypto
review found no blocking defect in the frozen export implementation.

The first integrated Linux workspace run passed the CLI, agent, connector, core
and server suites but failed six backup tests: their temporary-directory fixture
inherited0755 permissions under Linux's umask. The exporter correctly rejected
those paths. The fixture now creates0700 directories explicitly; production
permission checks were not relaxed. The affected store suite then passed8 tests
(one explicit reference test skipped), followed by the remaining xtask and
workspace doc-test gates. These initial failures remain part of the evidence;
this is not described as a single clean full-workspace run.

The native keygen extension and shared no-overwrite publication helper passed
additional independent review and affected Linux checks:8 store tests and2 CLI
tests, with2 reference cases skipped by default. Initial SFTP source transfer
timed out before execution; verified SSH transfer succeeded for the test run.
Standard reference age interoperability is measured on macOS. Linux checks use
the pinned Rust1.89 image with no network, nonroot execution, read-only verified
source and vendored dependencies; they do not change a production installation.

Private staging/partial files have best-effort cleanup on normal/error return. IO failure or OS kill can
leave owner-private residue; secure erasure is not promised. Limits and directory
separation are documented in [backup](../backup.md). Since identity paths are not
persisted, separation from a future output directory is the operator's
responsibility, while fixed archive entries always exclude identity files.
Historical deletion replay, independent latest-head trust/freshness, restored
work/provider/accounting quarantine and actual authoritative media recovery remain
unimplemented and the full runtime-recovery tasks stay open. No real credentials,
provider/model calls, deployment, original services/cron/DB changes or CI enablement.
