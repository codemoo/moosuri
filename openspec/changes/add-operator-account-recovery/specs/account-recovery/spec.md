## ADDED Requirements

### Requirement: Explicit stopped-installation account enrollment
The local operator MUST select an existing journal-locked account UUID and current
role explicitly. The supplied credential MUST differ from the stored revoked
password. Recovery MUST require the exclusive installation lock and MUST
NOT reset healthy or nonexistent accounts or initialize a missing installation.

#### Scenario: Recover a journal-locked account
- **WHEN** the operator supplies a new private-file credential for an account locked by startup replay
- **THEN** a new durable auth epoch, explicit role and 24-hour temporary credential commit atomically with session revocation and the journal head
- **AND** login requires password exchange before private APIs and issues no session during the recovery command

### Requirement: Interrupted recovery remains fail closed
Recovery MUST fsync its opaque auth record before committing credential changes.
Failure after append MUST poison the actor until reopening; startup MUST revoke
old authority and require another explicit enrollment if the DB commit is absent.

#### Scenario: Interruption before the credential transaction commits
- **WHEN** the new auth record is durable but its credential transaction fails
- **THEN** old sessions and old or attempted credentials remain unusable after restart
- **AND** a retry requires a fresh enrollment at a higher auth epoch

#### Scenario: Response lost after commit
- **WHEN** the credential transaction commits but the operator does not receive its response
- **THEN** reopening preserves the committed temporary credential and another recovery cannot reset the now-healthy account

### Requirement: Private credential input and authority isolation
The CLI MUST read bounded owner-only regular single-link password files without
following links or blocking on special files. It MUST NOT expose the credential
in argv, output or diagnostics, rewrite old work/connector epochs, or change other
accounts. Explicit account recovery MUST NOT be described as backup/restore.

#### Scenario: Recover an old administrator as a member
- **WHEN** the operator explicitly selects the member role
- **THEN** the previous administrator role and sessions do not regain authority and another user's account and data remain usable
