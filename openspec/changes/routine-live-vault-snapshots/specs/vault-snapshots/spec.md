## ADDED Requirements

### Requirement: Non-sealing live writer-consistent capture
An opt-in local worker MUST capture DB/WAL and full journal under the same exclusive
Database mutex used by HTTP, then release it before semantic replay/encryption.
It MUST NOT seal, retire or change sessions/source authority. Outputs MUST remain
bounded/private/new and pin their exact archive and captured journal head.

#### Scenario: Source changes after capture
- **WHEN** the live source commits more work after capture and before encryption
- **THEN** the captured snapshot remains unchanged and source append/serving continues

### Requirement: Distinct explicit snapshot recovery
Snapshot recovery MUST reject sealed-current mode and vice versa. It MUST validate
age EOF, file/head/schema/semantic evidence, quarantine all imported sessions and
pending authority, and publish only an absent root with a fresh service identity.
Existing completed matching restore retries MUST preserve subsequent work.

#### Scenario: Captured pending operation
- **WHEN** the selected snapshot contains a pending ciphertext operation
- **THEN** restore cancels it with its exact reservation/body and cannot execute it
  through an imported session or stale commit receipt

### Requirement: Truthful captured coverage
The snapshot/head MUST NOT imply independent latestness, source retirement,
post-capture deletion coverage, off-host durability or zero loss. Combining an older
snapshot with newer independent journal evidence MUST require a separate contract.

#### Scenario: Owner deletion after an older snapshot
- **WHEN** an operator explicitly restores that older point-in-time snapshot
- **THEN** later deletion coverage is not claimed; a snapshot capturing the deletion
  preserves the actual deletion fence and rejects owner resurrection
