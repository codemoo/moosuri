## ADDED Requirements

### Requirement: Guarded composite start identity
An optional start intent SHALL bind client UUID/expected generation, current
owner/connection/config/grant binding and exact before-sequence/checkpoint. Store
SHALL recheck current authority inside the allocation transaction. Exact retained
replay SHALL return the original run identity/state without renewal or a new
generation. A changed binding/payload or occupied different intent SHALL fail.

#### Scenario: Start commits before its ACK is lost
- **WHEN** the producer repeats its persisted start intent after the server committed it
- **THEN** the same run ID/generation is returned and one source batch can complete
- **AND** another active run is not borrowed

#### Scenario: Token or scope changes
- **WHEN** auth/credential/config/before-position or expected generation differs from the saved intent
- **THEN** a new start allocation fails without changing data/cursor or adopting history

### Requirement: Pruning and compatibility preserve the fence
Start metadata SHALL follow normal retained run history, while monotonic connection
generation SHALL prevent pruned old intents from allocating again. UUID alone
SHALL NOT be promised as a perpetual global identity. Legacy `{}` clients and
ordinary official poller starts SHALL retain their existing behavior. Restore
SHALL quarantine grants/history without resetting the generation fence.

#### Scenario: Unreferenced history is pruned
- **WHEN** an old run/start mapping is pruned and its composite intent is repeated
- **THEN** the old expected generation fails rather than creating a new run
- **AND** an explicit new generation remains available without a permanent key quota

#### Scenario: Existing clients and sealed restore
- **WHEN** old clients use `{}` or a sealed-current snapshot is restored
- **THEN** old start responses remain compatible and restored old grants cannot reactivate their starts

### Requirement: Honest producer retry and inactive history
The producer SHALL persist immutable start intent before dispatch. A later explicit
invocation may create one fresh attempt only with known inactive original identity,
no staged batch envelope, absent original batch receipt, exact current before-position
and no conflicting active run. Otherwise it SHALL retain state and stop. Unknown
historical runs SHALL not be rewritten as successful.

#### Scenario: Startup or expiry makes the acknowledged start unknown
- **WHEN** a later invocation finds its exact historical start inactive before batch staging
- **THEN** one fresh pinned attempt can collect the original snapshot without duplicate import
- **AND** the old unknown run remains unknown

#### Scenario: An inactive attempt already staged its batch
- **WHEN** that attempt has no confirmed original receipt
- **THEN** its immutable envelope remains pending and is not rebound under a new key
