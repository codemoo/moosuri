## ADDED Requirements

### Requirement: Authenticate bounded independent encrypted archives
The client SHALL export only encrypted v2 header/items with a domain-separated authenticated manifest binding header, item count, exact ordered ciphertext payloads and format version. It SHALL validate exact bounds and policies before KDF and authenticate the manifest and every payload before revealing preview or installing import state. It SHALL open source archives locally with source password or recovery without an original-server request or fabricated storage receipt.

#### Scenario: Corrupt, reordered, truncated or excessive archive
- **WHEN** an archive has a changed header/payload, reordered/duplicated/missing item, count mismatch, trailing data, wrong secret or excessive bound/KDF policy
- **THEN** it fails closed with no source preview/staged import and no destination mutation

### Requirement: Import selected rows into the current unlocked owner
The client SHALL show ordinal duplicate/new/changed statuses, create fresh IDs and keys under the current owner/key, preserve all fields including TOTP and use immutable per-item operations and exact receipts for partial/lost-ACK recovery.

#### Scenario: Lost second import acknowledgement
- **WHEN** one selected item has committed and the next durable acknowledgement is lost
- **THEN** the first remains committed, the second exact operation/ciphertext remains pending, reconciliation advances only from its exact receipt, and the UI shows committed/remaining counts

### Requirement: Fence archive and destination lifetimes
The client SHALL reject changing export snapshots, dispose late file/read/secret results after lock/account/key-head change, allow lock to interrupt source KDF, and preserve acknowledged destination items after cancellation. It SHALL explain that historical archive wrappers and original secrets survive later protection changes and that malicious served JavaScript remains a threat.

#### Scenario: Destination protection changes while source file is read
- **WHEN** destination header generation/envelope commitment changes before a delayed source file read returns
- **THEN** the Worker locks and no preview or secret result reaches the retired UI generation
