## ADDED Requirements

### Requirement: Large external attachments do not consume body-read MIME bytes
The system SHALL read supported Gmail bodies from bounded full-message MIME trees,
expose exact attachment metadata and fetch selected attachment bytes only through
the official fixed-origin message attachment endpoint.

#### Scenario: A message has a PDF larger than the former raw-message bound
- **WHEN** a supported body accompanies a separately stored PDF over512KiB
- **THEN** body reading succeeds without downloading the PDF and exact metadata permits a bounded separate text read

#### Scenario: A part identifier is forged or MIME coverage exceeds limits
- **WHEN** message/part/attachment identity fails validation or MIME structure exceeds declared bounds
- **THEN** no substituted document or arbitrary URL is read and the operation fails explicitly

### Requirement: Attachment text reads have bounded coverage and stable provenance
The system SHALL support PDF text through the deployment-owned constrained child
and UTF-8 text/plain, expose partial/page bounds and actual binary/text digests,
and require the combined exact-document digest for continuation pages.

#### Scenario: A PDF is read through the actual fixed child
- **WHEN** a supported PDF within2MiB/50pages/24KiB text is selected
- **THEN** the result contains its real extracted page text and provenance in a bounded durable read receipt

#### Scenario: Extraction is incomplete or unsupported
- **WHEN** a PDF is encrypted, scanned, malformed, unsupported, over limit or no extractor is configured
- **THEN** an explicit failure status is returned without claimed document text or suspension of otherwise valid OAuth

#### Scenario: A continuation would cross document versions
- **WHEN** binary content or extracted text differs from the requested combined digest
- **THEN** the result requires restarting and does not splice another page into the previous document

#### Scenario: Valid body and metadata exceed the aggregate tool envelope
- **WHEN** a supported message or three-message thread includes long filenames/IDs, escaped body text and large display headers/lists
- **THEN** the whole serialized result remains bounded, saves an actual external receipt, marks partial fields and exposes exact remaining attachment-list continuation without shortening provider identifiers

### Requirement: Attachment reads preserve current mail authority and revocation
The system SHALL route attachment reads through existing owner/mission read grants,
epoch revalidation, durable external receipt accounting and mail dependency purge.
No attachment read SHALL authorize attachment sending or execute document instructions.

#### Scenario: A mailbox is disconnected during download or after a receipt
- **WHEN** the owner disconnects before the private result returns or after derived output was recorded
- **THEN** late data is withheld and existing derived output is revoked

#### Scenario: A durable connected mission lacks an interactive session
- **WHEN** it reads an attachment of a freshly observed message under its admitted owner grant
- **THEN** the real read grant is used without manufacturing a user session, while unobserved/foreign targets remain fenced

### Requirement: Gmail collection consent remains body-only
The system SHALL preserve source account/label/time scope and history pagination,
index only supported body text, and never claim attachment/PDF indexing from this path.

#### Scenario: Collection encounters a large external PDF
- **WHEN** a scoped message has supported body text and an external PDF attachment
- **THEN** the source page records body text and original message metadata without downloading or indexing the PDF
