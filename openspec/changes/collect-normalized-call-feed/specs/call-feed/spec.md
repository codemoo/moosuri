# Call feed

## ADDED Requirements

### Requirement: Explicit settled normalized source
The command SHALL require an explicit private root, exact destination connection,
ingest token and private state. Versioned normalized call identity, timestamp,
settled marker and literal attributed transcript SHALL be required. Declared audio
hashes SHALL match exact anchored local bytes. Changed/unsafe/unsettled files SHALL
defer and local absence SHALL NOT authorize a source deletion.

#### Scenario: A changed export settles before ingestion
- **WHEN** a selected call file changes during its read or remains within the settle interval
- **THEN** the producer defers it without publishing a partial source
- **AND** a later stable export updates the same call identity while local file absence preserves previously committed sources

### Requirement: Complete bounded transcription with unknown fences
Optional owner-selected fixed Audio API transcription SHALL accept only validated
canonical WAV≤20minutes in≤120-second chunks, within ten request attempts per
invocation. Each chunk intent SHALL be durable before dispatch, confirmed results
SHALL be reusable and unknown chunks SHALL NOT be resent. Only complete transcripts
SHALL be published; overflow SHALL fail explicitly.

#### Scenario: An uncertain later audio chunk blocks complete publication
- **WHEN** an earlier chunk has a confirmed transcript and a later chunk has an uncertain provider result
- **THEN** a repeated invocation reuses the confirmed result and does not resend the uncertain chunk
- **AND** the producer does not publish a partial transcript as a complete call

### Requirement: Explicit retry of definite rejection
Definite pre-dispatch failures and explicit401/403/429 rejection SHALL be durably
retryable only by a later explicit invocation. Duplicate descriptors SHALL NOT
cause same-scan retries. First/latest rejection identity and exact rejection count
SHALL remain bounded private history after a later successful intent.

#### Scenario: A rejected credential is repaired
- **WHEN** the provider rejects a chunk with401 and the owner fixes the key before a later explicit invocation
- **THEN** the later invocation may create one fresh durable intent and retain the original rejection identity
- **AND** duplicate descriptors in the earlier invocation do not cause an immediate retry

### Requirement: Per-owner provenance and extraction exclusion
Existing owner connection/run/batch/request/cursor/revision receipts SHALL govern
source commit and replay. Exact raw/audio/transcript hashes SHALL remain evidence.
Unknown/mixed speaker markers SHALL be rejected by both automatic-memory attribution
modes and SHALL NOT be accepted as an owner-specified or exporter-claimed speaker.

#### Scenario: Mixed speakers cannot become a personal fact
- **WHEN** an imported call has unknown or multiple reported speakers
- **THEN** its transcript remains source evidence for the destination owner
- **AND** automatic-memory extraction skips it under both reported and collection-specified attribution without spending a model call
