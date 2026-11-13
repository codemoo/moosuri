## ADDED Requirements

### Requirement: Separate ciphertext authority
The vault SHALL use a separate process, DB/journal, cookie and origin with a deployment contract for a separate OS UID. Assistant account login SHALL authenticate only a single-use short-lived handoff and SHALL NOT unlock vault contents. Every private request SHALL validate its original session/account/tenant/epoch/installation online through a service-authenticated narrow adapter and fail closed on unavailability.

#### Scenario: Current logout or suspension
- **WHEN** the underlying session is logged out, reset or suspended before new admission
- **THEN** new vault requests fail and no administrator role supplies a decrypt capability

#### Scenario: Other account and hostile origin
- **WHEN** a cookie is used with another account header or a form is posted from a nonconfigured Origin
- **THEN** the vault rejects the request without returning another owner's ciphertext or reflecting supplied plaintext

### Requirement: Exact immutable ciphertext mutations
Prepared key and item candidates SHALL remain inactive until exact current-owner commitment and local CAS adoption. Header/inventory/item revision checks and persistent nonce reservations SHALL cover retained/aborted candidates. Lost ACK reconciliation SHALL reuse the same immutable operation. Journal/DB identity, sequence or head mismatch SHALL prevent opening.

#### Scenario: Rotation cancellation and lost acknowledgement
- **WHEN** a rotation is cancelled or its committed ACK is lost
- **THEN** cancellation retains the prior usable head, and exact retry of the committed operation yields one adopted inventory without duplicate encryption or activation from a mismatched receipt

### Requirement: Worker-owned local secrets
The browser SHALL use the actual Rust wasm32 crypto core in a dedicated Worker with injected browser CSPRNG and no weak fallback. Keys, CSV and pending crypto state SHALL remain in the Worker. Lock/account changes SHALL terminate it and fence late callbacks. Private browser storage/caching SHALL NOT persist the vault's secrets or source CSV.

#### Scenario: Late reveal after lock
- **WHEN** a reveal is delayed while the user locks the vault
- **THEN** its callback cannot restore plaintext in the UI and a fresh Worker is required for unlock

### Requirement: Usable bounded encrypted flows
The client SHALL provide setup/recovery verification, wrong-secret feedback, encrypted CRUD/search/favorites/expiry, secure password generation, explicit timed reveal/copy, local TOTP, bounded generic-filter CSV and persisted key-change CAS. CSV partial commits SHALL remain stable across lost ACK reconciliation and duplicate preview SHALL not disclose private row values to the server.

#### Scenario: Interrupted local import
- **WHEN** a selected import's commit ACK is lost after earlier rows saved
- **THEN** the client reconciles the same operation, resumes the encrypted batch without duplicates and does not transmit source CSV/plaintext fields

### Requirement: Explicit incomplete boundaries
This increment SHALL NOT claim credential broker execution, encrypted client archives, standalone vault backup/restore, old-v1 compatibility/migration, account deletion across two DBs, signed independent client, real mobile support or production deployment merely from browser/native tests.

#### Scenario: Server-delivered client replacement
- **WHEN** the server can replace the delivered JavaScript
- **THEN** the UI and contract retain the malicious-client-code limitation despite ciphertext-only storage

### Requirement: Current key lifetime admission
Every new local secret operation and emitted result SHALL compare an authenticated vault-local current header commitment with the Worker admitted head, retaining the original assistant owner/session/epoch check. Password, recovery and root changes SHALL retire old honest-client Workers and clear their UI. Background checks SHALL serialize with own-commit adoption. Inventory-only edits SHALL NOT unnecessarily retire the key lifetime.

#### Scenario: Peer changes keys while a reveal check is delayed
- **WHEN** a peer key change commits after an initial current-head response was admitted but before the result's final admission
- **THEN** the final head check blocks the new reveal and retires the old Worker

#### Scenario: An own committed key ACK is lost
- **WHEN** the current head is the exact locally owned committed candidate and a background check observes it before acknowledgement
- **THEN** only a Rust-verified exact operation/payload/header/commitment/inventory receipt permits settlement retention, without arbitrary header admission or key activation

#### Scenario: Independent items commit concurrently
- **WHEN** a valid exact committed receipt includes a peer's independent same-header item beyond the cached inventory
- **THEN** the client verifies its own operation effect, acknowledges the save and requires refreshed state instead of retrying that committed operation forever

#### Scenario: Peer changes after final output admission
- **WHEN** bytes have already been admitted by the final authenticated current-head read before a later peer key change
- **THEN** later arrival is documented as already admitted bytes and every subsequent new secret operation still checks the current head; retroactive erasure is not claimed
