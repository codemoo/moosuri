## ADDED Requirements

### Requirement: Owner-scoped immutable UTF-8 sources
The system SHALL accept only nonempty TXT/Markdown/CSV/JSON files up to24KiB whose actual content is valid UTF-8 without NUL or control characters other than LF/CR/TAB; JSON SHALL parse. Original bytes, digest and revision SHALL be immutable and available only through current owner authorization.

#### Scenario: Administrator cannot read a member original
- **WHEN** an administrator requests a member file preview or content
- **THEN** the system returns no private content.

#### Scenario: Unsupported binary is explicit
- **WHEN** a user uploads PDF/audio/Office, invalid UTF-8, invalid JSON or oversized input
- **THEN** upload fails explicitly without claiming text extraction.

### Requirement: Source evidence is data with revocable provenance
The system SHALL bind selected exact source IDs/revisions to chat and retry, supply original digest and UTF-8 byte evidence, and treat source content as untrusted data without tool or persistent-write authority. The system SHALL preserve selected evidence and current user text while trimming only complete oldest conversation turns when protocol limits require it, disclosing omitted history.

#### Scenario: A file question reaches actual source bytes
- **WHEN** a user selects a file and asks about content
- **THEN** the model receives that immutable text with ID/revision/SHA256 and exact byte interval as a labelled data message separate from the current user request.

#### Scenario: Worst-case escaped sources fit the model protocol
- **WHEN** four full valid files consist of quotes and backslashes
- **THEN** the real encoder/runtime supplies all selected bytes without exceeding message/request caps and preserves a contiguous suffix of prior complete turns.

#### Scenario: Document instructions cannot issue a write
- **WHEN** document text asks for memory, artifact, reminder or external writes
- **THEN** document-linked chat has no tool/write authority.

#### Scenario: Retry and withdrawal retain exact association
- **WHEN** an ACK is lost or a model round fails
- **THEN** explicit retry uses the original immutable source refs; deletion revokes dependent answers and prevents later output or retry from reviving source content.

### Requirement: Independent safe file experience and restoration
The system SHALL provide independent upload progress/cancel/retry, safe text previews, original downloads and deletion without blocking other messages or images. Private transport SHALL be no-store and fenced by account/deletion lifetime. Completed originals/extraction and tombstones SHALL survive verified encrypted restore while pending leases are quarantined.

#### Scenario: File upload does not wait for image preparation
- **WHEN** image preparation is stalled and a file is selected
- **THEN** the file uploads independently and sending text leaves incomplete attachments selected.

#### Scenario: Markup and filename remain plain text
- **WHEN** a filename or text looks like active HTML/Markdown
- **THEN** preview renders text only and downloads use a fixed attachment/nosniff header.

#### Scenario: Deleted or old-account pending transport returns late
- **WHEN** preview/download/upload returns after deletion or account switch
- **THEN** it cannot reveal content or recreate a selected attachment.

#### Scenario: Sealed restore preserves completed data and quarantines pending work
- **WHEN** a verified encrypted current snapshot is restored with fresh enrollment
- **THEN** ready owner originals/extraction remain, deleted bytes stay absent, cross-account reads fail and pending upload settlement is denied.

#### Scenario: Reopened pending upload keeps its original identity
- **WHEN** a user explicitly reselects the original file on a retry-required library row
- **THEN** upload reuses that row UUID and server digest/filename validation; a late completion after deletion cannot restore the row.
