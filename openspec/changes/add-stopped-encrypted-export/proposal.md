# Why

The development application has no portable encrypted export. A supported
historical restore additionally needs durable deletion coverage and independently
maintained latest recovery inputs, so export must not claim those later gates.

# What Changes

- Add stopped-installation public-recipient setup with an operator-held X25519
  identity challenge and durable public-only installation/key-epoch metadata.
- Export a verified SQLite Backup snapshot and both snapshot journals in a fixed
  tar payload encrypted using the established age-v1 format.
- Enforce bounded private file IO, existing writer admission, no source mutation
  on rejection, explicit byte hashes/heads and atomic no-overwrite publication.
- Retain old public epochs and document independent private-key retention.

# Impact

Rust store/CLI and installation migration0013, age/tar dependencies and rusqlite
Backup feature. No restore command, external key escrow, runtime workers/provider
calls, authoritative media support or unattended/latest-journal service. The
complete-runtime-recovery-contracts implementation tasks remain pending.
