## ADDED Requirements

### Requirement: Permanent administrator deletion
The system SHALL require current unrestricted administrator authority and exact
account management revision, reject self deletion and removal of the last usable
administrator, and durably journal deletion before revoking sessions and cleanup.
Deleted accounts SHALL NOT resume or recover through password recovery. The same
request SHALL reconcile the same receipt without a second destructive action.

#### Scenario: Owned data removal with unavailable vault
- **WHEN** another administrator deletes an account while the vault is offline
- **THEN** that account immediately loses access, owned active content/credentials
  are purged or explicitly cleanup pending, other users/global providers survive,
  and vault delivery remains pending until a durable exact acknowledgement

### Requirement: Idempotent lifecycle delivery and recovery
The assistant SHALL retain a durable opaque lifecycle outbox. The authenticated
vault service SHALL fsync its owner-scoped lifecycle intent and commit its purge
and tombstone before acknowledging the exact event. Deletion SHALL fence all
later owner epochs and late operation adoption; revocation SHALL fence older
epochs. Safe monotonic lifecycle crash tails SHALL replay before admission.

#### Scenario: Lost acknowledgement and old snapshot
- **WHEN** the vault commits deletion but the ACK is lost, or older current data
  is reopened with the retained latest deletion journal
- **THEN** retry SHALL preserve the same event and deletion fence without harming
  another owner or restoring deleted content/credentials

### Requirement: Honest retained history
The UI and documentation SHALL distinguish local cleanup, vault ACK and retained
encrypted historical journal/checkpoint bytes. Minimum opaque accounting and
external-effect state MAY survive; unknown dispatch outcomes SHALL NOT be called
cancelled. Active logical deletion SHALL NOT imply provider remote deletion,
physical media secure erase or erasure of copies already exported to the user.

#### Scenario: Completed logical deletion with retained encrypted backup
- **WHEN** local cleanup and vault delivery have completed but historical encrypted copies remain
- **THEN** People SHALL show completed delivery separately from historical retention
- **AND** documentation SHALL preserve the limits on physical erasure and remote effects
