## ADDED Requirements

### Requirement: Current administrator controls access
Only a fully authenticated current administrator SHALL change another account's
status, role or temporary password. Every change SHALL invalidate old sessions
and background authority without giving the administrator access to private data.

#### Scenario: Suspend then resume a member
- **WHEN** the administrator suspends and later resumes a member
- **THEN** suspension blocks new and existing login, saved data remains, and only a fresh valid login can use the resumed account

#### Scenario: Administrator authority changes during hashing
- **WHEN** the administrator is demoted while a reset is prepared
- **THEN** the final commit rejects the stale authority without changing the target

### Requirement: Revision-pinned and idempotent management
Management SHALL compare the expected target revision and retain a content-free
receipt bound to the administrator, target, request UUID and exact input digest.
Only the first committed reset response SHALL disclose its temporary password.

#### Scenario: Lost reset acknowledgement
- **WHEN** the same reset request is retried after commit
- **THEN** it returns the original receipt with no newly generated or replayed password

#### Scenario: Target changes independently
- **WHEN** the target changes its password after preparation
- **THEN** the stale reset conflicts before journal adoption and preserves the new password

### Requirement: Crash-safe revocation and no accidental reactivation
An adopted epoch SHALL remain a revocation across a failed database commit.
Startup SHALL require durable validated journal bytes and lock the affected
account for local recovery rather than infer the interrupted action succeeded.

#### Scenario: Database commit fails after journal adoption
- **WHEN** the management transaction fails after append and fsync
- **THEN** the actor stops accepting work and restart rejects the old account authority while preserving unrelated accounts

### Requirement: Account-scoped management UX
The people screen SHALL preserve current selection, perform actions without
redundant confirmations, and fence old-account, duplicate and stale-list responses.

#### Scenario: Account switches while a password response is in flight
- **WHEN** a late response contains a password for the former account lifetime
- **THEN** the new account neither displays it nor changes its own login state
