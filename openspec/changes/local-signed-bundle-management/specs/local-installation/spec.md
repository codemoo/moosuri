## ADDED Requirements

### Requirement: Pinned local native bundle installation
The Rust CLI SHALL install a bounded local native bundle only after exact-byte Ed25519 signature verification against an explicit independently supplied public pin, manifest target/version/schema checks and exact binary size/hash verification. The copied executable SHALL report matching public metadata and embedded React assets. Installation SHALL NOT create accounts or execute deployment hooks.

#### Scenario: Fresh local installation and init
- **WHEN** an operator supplies a matching signed native bundle and explicit public pin
- **THEN** the CLI SHALL copy verified executable/assets into the new private managed prefix and generate reviewable deployment files
- **AND** explicit managed init SHALL reuse hidden/password-file administrator creation, and the resulting server SHALL serve embedded React without Node/Python.

#### Scenario: Malformed or substituted bundle
- **WHEN** signature, hash, native target, fixed binary basename or public executable metadata differs
- **THEN** activation SHALL reject without executing unverified code or changing private data.

#### Scenario: Private installation paths
- **WHEN** runtime or data roots are supplied
- **THEN** they SHALL be disjoint private absolute paths; symlinks, traversal, control characters and unrelated existing runtime prefixes SHALL reject.
- **AND** private opened files/directories SHALL reject extended/inherited ACL entries, and trusted ancestors SHALL reject grant ACLs without silently stripping operator ACLs.

### Requirement: Stopped same-schema update and recoverable binary activation
An update SHALL require a strictly increasing signed release sequence, nondecreasing SemVer, unchanged trust pin and exact current Store schema. It SHALL hold runtime-exclusive admission and the original existing stopped writer lock continuously through integrity/journal verification, encrypted stopped export and fsynced binary activation. No live database migration or service start SHALL occur.

#### Scenario: Running writer
- **WHEN** a managed server or another data writer remains active
- **THEN** update SHALL fail without activating a binary or creating a misleading success receipt.

#### Scenario: Interrupted activation
- **WHEN** active selection is about to change
- **THEN** a durable pending record SHALL first preserve the old/new signed releases.
- **WHEN** interruption occurs before or after active-state rename while the pending record remains
- **THEN** managed startup SHALL reject
- **AND** explicit stopped recovery SHALL verify recorded slots/current state and select only old or new binary without rolling back SQLite.

### Requirement: Exact managed uninstall and explicit deployment
Default uninstall SHALL require stopped admission and remove only exact unchanged managed runtime files and verified release files. It SHALL retain private data, backups, unrelated entries and changed configuration without recursive home/data deletion. Deployment templates SHALL be reviewable only: no actual service, Tailscale Serve/Funnel, DNS or proxy change SHALL run implicitly.

#### Scenario: Retained operator data and custom files
- **WHEN** an operator uninstalls a stopped managed runtime containing an unrelated note or modified review file
- **THEN** private SQLite/data and those files SHALL remain, while exact managed executable/state files are removed.


#### Scenario: Mode-compatible direct Tailscale topology
- **WHEN** the operator chooses direct Tailscale Serve mode
- **THEN** the generated service SHALL omit forwarded-IP trust so missing X-Real-IP or chained X-Forwarded-For cannot prevent login.
- **AND** account login SHALL use actual loopback peer+email attempt buckets, without claiming individual tailnet-client IP attribution.
- **WHEN** the operator chooses HTTPS/Nginx mode
- **THEN** the supplied proxy SHALL overwrite exactly one X-Real-IP matching the generated server’s single-IP parser.
