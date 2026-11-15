## ADDED Requirements

### Requirement: Explicit external-grounded document
A newly admitted capable run SHALL require the current user's isolated single-document create/edit grant before exposing configured public HTTP and current-owner pinned Gmail/primary Calendar reads. The grant SHALL NOT expand effect or persistence authority.

#### Scenario: Research is saved rather than merely promised
- **WHEN** the current user requests one report based on supported external reads
- **THEN** actual admitted read receipts ground one document at its current revision
- **AND** mutation, fixed version confirmation and Completed state commit together
- **AND** a lost acknowledgement cannot create another version or model dispatch

#### Scenario: Save a completed read in the same conversation
- **WHEN** an earlier owned completed read answer is actually selected into context and the current user explicitly requests a document
- **THEN** the exact earlier supported receipts become provenance of the new document
- **AND** previews, recalled conversations, unsupported or unknown reads cannot silently become saved evidence

#### Scenario: Unsupported context or old admission
- **WHEN** a run requests document persistence from Calendar command/action context or inherited connection dependencies without successful supported read receipts
- **THEN** persistence is refused
- **AND** the document grant admits no drafts, sends, Calendar actions, browsers, native credential previews or conversation recall
- **AND** older runs receive no new external-document capability

### Requirement: Durable receipt provenance and safe revision
The Store SHALL derive at most64 exact receipt identities per version from actual originating run/step/call, typed request and request/result SHA256. Callers SHALL NOT supply external authority or provenance. Ordinary edits SHALL preserve valid sources without a document/writer self-revocation cycle.

#### Scenario: Ordinary owner revision
- **WHEN** the owner edits an externally grounded document through an anchored command or editor
- **THEN** the revision preserves its exact existing provenance
- **AND** version replacement retires obsolete write-body/checkpoint/confirmation projections without revoking independent valid read receipts

### Requirement: Transitive external evidence withdrawal
Actual source/connection/run withdrawal, source-thread deletion and restore SHALL withdraw derived content through common purge contracts. Withdrawal/deletion SHALL erase typed request plaintext while retaining opaque receipt identities and hashes.

#### Scenario: Owner edits and later disconnects an evidence account
- **WHEN** an owner revises an externally grounded report and later disconnects the underlying account
- **THEN** the revision preserves the original exact receipts until disconnection
- **AND** dependent body/title/payload/history/export and downstream answers are withdrawn with no old-version export

#### Scenario: Cancellation races a read or save
- **WHEN** the original run is cancelled or its evidence is withdrawn before commit
- **THEN** a late read or draft cannot commit a document
- **AND** another account cannot read, export or derive from the original owner's evidence

#### Scenario: Restore contains previously saved external text
- **WHEN** an installation containing externally grounded versions is restored
- **THEN** those versions and their dependent projections are withdrawn before serving
- **AND** reopening cannot resurrect cached text, reconnect or automatically rerun external work
