## ADDED Requirements

### Requirement: Personal versioned records
The system SHALL persist documents, goals and followups under the authenticated
tenant, with immutable kind, closed payload, head revision, bounded history and
content-free idempotent receipts. UI edits SHALL accept at most32KiB Markdown.

#### Scenario: Editor saves and revises a report
- **WHEN** an authenticated user creates and later edits a report at its expected head
- **THEN** one identity has two versions and the actual receipt links to that identity
- **AND** a repeated request returns the same receipt while altered payload conflicts

### Requirement: Current-request artifact authority
The system SHALL pin schema1 artifact capabilities only at new run admission.
A write SHALL require a sealed current-user intent before retrieval, validated
account/lease, exact target/head, complete current read dependencies and fixed UUID.
Chat body SHALL be at most2048 bytes and the entire existing slot limit4096 bytes.
Edits SHALL replace one unique literal anchor of at most512 bytes.

#### Scenario: Source instruction cannot grant a mutation
- **WHEN** a read-only request retrieves text instructing artifact creation
- **THEN** artifact_write is absent and no artifact mutation is accepted

#### Scenario: An editor wins a concurrent revision
- **WHEN** a model holds an older artifact revision and the editor saves a newer head
- **THEN** the old run cannot overwrite it or repair its own CAS authority
- **AND** the UI retains its local unsent draft on conflict

### Requirement: Atomic saved confirmation
A chat artifact mutation, tool receipt, fixed confirmation and Completed state
SHALL commit together. A missing tool acknowledgement SHALL NOT cause another
model draft or artifact version after restart.

#### Scenario: Confirmation storage fails
- **WHEN** the database fails while saving the fixed confirmation
- **THEN** the artifact mutation and its receipt are rolled back together
- **AND** retrying the existing tool slot can commit exactly one record

#### Scenario: Tool acknowledgement is lost after commit
- **WHEN** the worker loses its response and the installation restarts
- **THEN** the conversation shows the committed receipt and Completed state
- **AND** the run cannot be requeued for a new model request or mutation

### Requirement: Transitive withdrawal and recovery
The system SHALL register reads before delivery and inherit all delivered
source/memory/artifact dependencies even when a model omits citations. Normal and
replay withdrawal SHALL purge dependent title/body/payload/history and private
runtime output/checkpoints, retain tombstones/receipts and prevent late writes.
User artifact deletion SHALL use the existing durable typed destructive journal.

#### Scenario: Withdraw a source while drafting
- **WHEN** evidence is withdrawn while a dependent model output is held
- **THEN** no late report commits and resting dependent versions cannot be exported

#### Scenario: Adopted delete precedes a crash
- **WHEN** DeleteArtifact is fsynced before its DB effect and the server restarts
- **THEN** effect, receipt and watermark converge without recreating a conversation

### Requirement: Safe exports and honest followup briefing
The system SHALL escape Markdown in preview/HTML export, prohibit active or remote
content, and reject withdrawn exports. Briefing SHALL return at most five valid
pending records with continuation/coverage, excluding resolved/cancelled, muted
and currently snoozed records. A local record SHALL NOT attest a message, future
monitoring or notification schedule.

#### Scenario: Script-like Markdown is printed
- **WHEN** a stored body includes raw HTML, script links or remote images
- **THEN** preview and print render it as text without execution or network fetching
