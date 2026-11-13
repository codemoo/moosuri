# Why

An interrupted password change can durably advance the auth journal before its
SQLite credential transaction. Startup correctly locks that account, but the
operator currently cannot enroll a new credential.

# What Changes

- Add a stopped-installation local CLI for an existing recovery_required account.
- Require an explicit account UUID and role, and a supplied owner-only password file.
- Fsync a new auth epoch before atomically replacing the credential with a
  24-hour temporary password, revoking sessions and recording the journal head.
- Preserve fail-closed startup replay and force normal password exchange at login.

# Impact

Rust store and CLI, synthetic fault/installation checks and recovery documentation.
No HTTP reset endpoint, schema/dependency change, account disable/delete bypass,
encrypted backup/restore support or production deployment.
