## ADDED Requirements

### Requirement: Bounded immutable private image upload

The system SHALL validate actual PNG/JPEG/WebP bytes, dimensions and decode allocation; normalize images without private metadata; enforce2MiB canonical bytes, four selected images and64MiB account quota; preserve UUID/digest retry identity; and keep binary bytes outside metadata DTOs and logs. Browser originals SHALL be bounded to32MiB/48MP and prepared at2400px or less before upload.

#### Scenario: Independent upload and lost acknowledgement

- **WHEN** a file is still uploading and its owner sends unrelated text
- **THEN** only ready selections are bound, pending files remain, and retrying an upload uses the same UUID and prepared bytes
- **AND** the same UUID with conflicting bytes is rejected

### Requirement: Explicit image understanding and read-only authority

The system SHALL enable image input only after a successful unpredictable synthetic pixel probe at the current provider revision/model/effort. It SHALL send only server-loaded selected canonical bytes and SHALL withhold all tools and memory/artifact mutation for media-derived contexts. Unsupported combinations SHALL preserve text, selection and retries with actionable guidance.

#### Scenario: Synthetic provider observes pixels

- **WHEN** a positively verified provider receives an admitted image turn or explicit retry
- **THEN** the correct dialect carries the same immutable images and the existing accounted dispatch/identity/terminal guards remain active
- **AND** image instructions cannot create a tool or mutation capability

### Requirement: Revocable owner-scoped previews and restore

The system SHALL authenticate no-store binary reads, scope object URLs to the account lifetime, render actual private thumbnails in composer/bubbles/library, and journal media cancellation/deletion with dependent output withdrawal. Unfinished processing SHALL be quarantined after restart/restore; ready SQLite bytes SHALL remain available and restore SHALL not restart provider work automatically.

#### Scenario: Delete wins over queued or late work

- **WHEN** cancellation precedes upload admission or deletion races image processing/model output
- **THEN** its tombstone prevents revival, all dependent output/checkpoints/evidence are withdrawn and queued runs cannot be retried
- **AND** another tenant cannot fetch the bytes and account/deletion/lifetime changes revoke preview URLs
