## ADDED Requirements

### Requirement: Immutable private PDF source
The system SHALL accept bounded text PDFs only through the authenticated file upload path, retain original bytes and digest, and bind extracted text to the same immutable owner, revision and upload lease. Filename or document content SHALL NOT grant tool or persistent write authority.

#### Scenario: Delete wins while the parser runs
- **WHEN** an owner deletes a processing PDF before extraction settles
- **THEN** late completion cannot restore its bytes, preview or derived conversation context

#### Scenario: A different account requests the PDF
- **WHEN** another authenticated account presents the PDF identifier
- **THEN** the server refuses its original bytes and extracted text

### Requirement: Resource-limited pure Rust extraction
The system SHALL parse PDF text in a fixed deployment-owned Rust child with bounded original size, page count, decompression, text output, CPU and wall time and Linux address space. It SHALL neither execute active document contents nor access URLs/files named by the document.

#### Scenario: Scanned or unsupported PDF
- **WHEN** the PDF is encrypted, malformed, has no extractable text or exceeds supported bounds
- **THEN** the upload stays retryable with a fixed meaningful failure and no partial-ready claim

#### Scenario: Parser process stalls
- **WHEN** the child exceeds the wall time or output bound
- **THEN** the parent terminates the child and releases its upload slot without blocking other chat traffic

### Requirement: Honest preview and independent sending
The client SHALL show PDF extracted text as escaped source data with page markers, offer original download, and preserve drafts and original retry identity while uploads are in progress.

#### Scenario: Send while PDF is pending
- **WHEN** a user sends a message while PDF extraction has not completed
- **THEN** ready selected sources alone attach and the pending PDF remains available for later selection

#### Scenario: Source attribution
- **WHEN** the agent receives PDF text
- **THEN** its source metadata identifies the original digest/revision and describes offsets as extracted UTF-8 positions rather than PDF binary or visual coordinates
