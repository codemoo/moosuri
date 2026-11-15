## ADDED Requirements

### Requirement: Existing final preview authorizes exact native mail
The owner SHALL select a separate native send connection and use the existing full-recipient/subject/body/attachment-none/timing preview and explicit final approval. A model MAY prepare a local draft but SHALL NOT approve or send. Native connections SHALL be advertised/admitted only for drafting in the model's mail capability set. Matching local grants SHALL be reused without another routine approval.

#### Scenario: Edit or schedule change
- **GIVEN** a native mail action revision has been reviewed or assigned to a device
- **WHEN** recipients, body, connection or timing change before commitment
- **THEN** the old request cannot commit, the new revision is unapproved, and only its new final approval permits a distinct exact effect

### Requirement: Separate actual scope and native identity
The companion SHALL use a separately selected vault credential whose actual refresh scopes are exactly openid/email/gmail.readonly/gmail.send and whose subject/verified email/profile match the selected From identity. Existing read grants SHALL NOT gain send capability. Tokens SHALL NOT reach the assistant service, UI, model or tool output.

#### Scenario: Wrong account or actual scope
- **GIVEN** a fully approved action and native send selection
- **WHEN** actual refresh scopes, verified subject/email or mailbox profile differ
- **THEN** no send POST occurs and the original action settles without automatic retry or fallback

### Requirement: Immutable one-device execution and no replay
The writer SHALL bind approved MIME and owner/action/revision/connection/auth/timing/expiry to one device/binding/invocation and atomically create mail evidence and broker commitment before entry. Another broker SHALL NOT steal or fail over the effect. Existing read-broker session checks SHALL remain unchanged.

#### Scenario: Device enters the approved send
- **GIVEN** an exact committed request and bounded payload
- **WHEN** the device checks payload bytes, supported new plaintext/From identity, current native authority and signed online authority
- **THEN** it durably journals entry before the sole send POST

#### Scenario: Lost commitment or settlement ACK and restart
- **GIVEN** an adopted native effect
- **WHEN** an ACK is lost or either process restarts
- **THEN** recovery preserves the same invocation/commitment/receipt and never executes an entered send again; known non-entry is terminal and unknown delivery remains unknown

#### Scenario: Revocation and late authentic receipt
- **GIVEN** an approved native effect
- **WHEN** revocation precedes commitment or the final entry checks
- **THEN** new entry is blocked
- **AND WHEN** the provider was already entered
- **THEN** revocation cannot recall the mail and its authentic late receipt may settle only the original effect

### Requirement: Existing unknown-send reconciliation
Native unknown sends SHALL retain the original immutable Message-ID/fingerprint, bounded automatic/manual read budgets and separate observational result semantics. The existing conservative equality oracle and sticky/conflict merge SHALL be reused. A current authority check SHALL precede candidate raw retrieval. Lookup SHALL have no send authority and SHALL NOT replace unknown with a fabricated send receipt.

#### Scenario: Unique matching Sent observation
- **GIVEN** an original native send has an unknown provider outcome
- **WHEN** a complete unique Sent result matches the frozen full supported evidence
- **THEN** the action remains unknown, records the matching observation and makes no further send POST

### Requirement: Bounded projections and restore quarantine
Only validated provider message/thread IDs or bounded typed Sent-observation fields SHALL be projected; provider error text, tokens, raw MIME and arbitrary strings SHALL NOT be receipts. Native pending approvals/connections SHALL be quarantined on restore. Attachments, replies, HTML, arbitrary URLs/search and future-generated outgoing content SHALL remain unsupported.

#### Scenario: Untrusted credential echo or restored authority
- **GIVEN** a native payload/provider result or restored broker state
- **WHEN** credential material would cross the send/projection boundary or restored authority is not current
- **THEN** the operation fails closed without exposing secrets, reviving old approval or replaying a send
