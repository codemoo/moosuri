## ADDED Requirements

### Requirement: Select exact native preview message
The owner SHALL open only an opaque selector from their stored authenticated successful native preview. The device SHALL resolve it under the same device, preview binding, account/item/vault/epochs and original expiry. Arbitrary, cross-device, expired, revoked or changed selections and counts/preview-only grants SHALL NOT enter the provider.

#### Scenario: Separate body grant reused
- **GIVEN** a current preview selection and separately selected matching body grant
- **WHEN** the owner opens a body or confirms an uncertain response
- **THEN** the existing grant and original request UUID are reused without a new approval
- **AND** an entered operation is not replayed after restart

#### Scenario: Genuine original expiry
- **WHEN** the owner views a preview whose original request has expired
- **THEN** the saved preview remains displayable and body selection asks for a fresh preview
- **AND** existing invocation authority is not extended

### Requirement: Bounded literal plaintext only
The adapter SHALL repeat actual Google scope/identity checks and use the fixed selected-message GET. It SHALL project one bounded inline UTF8/US-ASCII text/plain part with explicit truncation. Deleted/non-INBOX results SHALL be unavailable; unsupported or malformed content SHALL be unknown. It SHALL NOT fetch attachments, links/images, raw MIME, arbitrary URLs or mail writes.

#### Scenario: Untrusted and credential-echo content
- **WHEN** the selected message contains markup, unsupported MIME or a credential echo
- **THEN** ordinary markup is rendered literally without action authority and an exact credential echo in the entire decoded text rejects before truncation
- **AND** malformed base64, invalid controls, unsupported charset/HTML-only and multiple plaintext parts reject

#### Scenario: Bounded projection
- **WHEN** the fixed provider response is decoded
- **THEN** JSON is limited to32768 bytes, the selected decoded part to16384 bytes, traversal to32 parts/depth4 and projection to4096 UTF8 bytes
- **AND** truncation preserves a character boundary and is disclosed

### Requirement: Private display without downstream model use
Selected bodies SHALL stay in signed private receipts and a separate owner UI in settings/original preview chat. They SHALL NOT enter model/chat history, recall, notifications or learning proofs. Credentials and provider message IDs SHALL remain device-local. Authentic late evidence may settle after revocation but SHALL NOT resurrect a displayed projection.

#### Scenario: Revocation and account change
- **WHEN** the device is revoked or the UI changes account/session lifetime
- **THEN** revoked results and retained previous-owner bodies are not displayed
- **AND** late asynchronous callbacks cannot restore an older UI lifetime
