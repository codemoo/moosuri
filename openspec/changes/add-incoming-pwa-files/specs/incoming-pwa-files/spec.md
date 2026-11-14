## ADDED Requirements

### Requirement: Bounded file handoff
The PWA SHALL accept supported photo/document/audio multipart inputs using the
existing intended-owner one-use handoff. It SHALL bound total files/bytes, audio count,
filename, type and each file's size and SHALL never put private bytes into public cache,
query strings or logs. The text-only handoff behavior SHALL remain available.

#### Scenario: File-only share and worker restart
- **WHEN** a supported file-only or mixed share commits before worker termination
- **THEN** exact bounded bytes survive until the intended account explicitly claims
- **AND** consumption erases stored payload and prevents replay under the same ID

#### Scenario: Unsupported or oversized input
- **WHEN** inputs exceed4 files/32MiB total/1 audio or a supported per-kind bound
- **THEN** staging fails with an opaque generic redirect and no upload/model call

### Requirement: Preview and explicit insertion
Review SHALL show local previews and permit file removal. Insertion SHALL preserve
the latest original-thread draft and preflight combined media capacity and active
recording. Shared files SHALL use existing attachment lifecycle after insertion;
shared audio SHALL require explicit conversion and SHALL not activate a microphone.

#### Scenario: Capacity conflict
- **WHEN** existing attachments or recording prevent insertion
- **THEN** the review remains available without overwriting prior work
- **AND** a capacity change after durable consume keeps only that original window's local review until the original expiry, with fresh authority checks on retry
- **AND** removing a shared file or finishing the existing work permits another attempt

#### Scenario: Audio conversion and independent chat
- **WHEN** the user inserts a shared audio file and photos into the original draft
- **THEN** audio remains local preview until explicit conversion, while photos upload
- **AND** ordinary text sending remains usable during preparation/upload/conversion

#### Scenario: Account retirement and stale delivery
- **WHEN** account/session/original-thread authority changes before final insertion
- **THEN** no stale bytes are inserted and retired preview URLs are released
- **AND** the existing fresh server proof, durable generation and one-use checks apply


#### Scenario: Compressed image with oversized dimensions
- **WHEN** an incoming image declares dimensions exceeding the existing48MP limit
- **THEN** header preflight rejects preview before creating an image URL or decoding
- **AND** the user can exclude the unsupported file without affecting ordinary chat
