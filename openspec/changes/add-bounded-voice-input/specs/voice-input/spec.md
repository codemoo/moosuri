## ADDED Requirements

### Requirement: Recorder lifetimes preserve chat and microphone ownership
The system SHALL offer permission-aware microphone recording, active elapsed time and input level, pause/resume, playable preview, re-record/delete and explicit interruption handling. It SHALL stop owned tracks and fence late callbacks on disposal/auth change/navigation/interruption. Recording, preparation, upload and transcription SHALL NOT block ordinary text composing or sending.

#### Scenario: Interruption and late microphone permission
- **WHEN** a recording is interrupted or its owner disappears before permission resolves
- **THEN** all returned owned tracks are stopped and no replacement account receives the recording
- **AND** returning to the app does not automatically resume recording.

### Requirement: Transcription is explicitly configured and server bounded
The system SHALL separately configure and verify an OpenAI API transcription binding using actual recognized speech before enabling it. It SHALL NOT infer audio support from text/image/tools/plan capability. The Rust upload boundary SHALL validate exact canonical PCM WAV bytes and sample-derived duration before dispatch and SHALL enforce current user/provider authority, concurrency and independent duration budget.

#### Scenario: Unsupported or forged audio
- **WHEN** recorder/decode support is unavailable or uploaded bytes violate the exact PCM envelope/duration bound
- **THEN** the system reports the real failure without claiming completed processing
- **AND** invalid input causes no external transcription dispatch.

### Requirement: Audio receipts fence retries and late completion
The system SHALL bind an ephemeral audio job to its original authenticated account/session, immutable input digest, provider revision/model and expiring nonce. It SHALL never automatically redispatch the same committed attempt. Restart/timeout after possible dispatch SHALL remain unknown, with conservative usage accounting.

#### Scenario: Lost acknowledgement, deletion, expiry or authority replacement
- **WHEN** the original upload acknowledgement is lost and the same UUID is retried
- **THEN** the existing receipt is returned without another provider request.
- **WHEN** deletion, expiry, session revocation or provider replacement precedes completion
- **THEN** a late response cannot restore or deliver its transcript.

### Requirement: Completed dictation remains an editable explicit draft
The system SHALL present the completed transcript as editable text and SHALL append it at most once to its original composer only after user insertion. It SHALL preserve later typed text and SHALL NOT send a chat request or invoke tools without the user's subsequent normal Send action. Audio and unpublished transcripts SHALL NOT enter browser persistent storage/public caches.

#### Scenario: User continues another text conversation during STT
- **WHEN** the user types/sends text or selects another thread while STT is pending
- **THEN** ordinary text remains usable and the result belongs to the original slot
- **AND** explicit insertion appends edited transcript without overwriting new text or automatically sending.
- **AND** successful insertion releases the preview and collapses the recording panel; the current original composer regains focus.

### Requirement: Audio configuration revocation is permanent for prior work
The system SHALL retain a monotonic transcription-authority generation while disabled. Probe completion SHALL compare-and-set the generation captured at admission. Disable/replacement SHALL atomically advance authority and cancel processing/ready jobs with their transcript erased. Re-selecting the same binding SHALL NOT reauthorize old probes/jobs.

#### Scenario: Held probe and same-binding re-enable
- **WHEN** a probe is held while audio is disabled or another selection commits
- **THEN** releasing the old response cannot enable or replace current settings.
- **WHEN** a ready/processing job predates disable and the same provider revision/model is verified again
- **THEN** its original receipt remains terminal and discloses no transcript.

### Requirement: Ambiguous browser transport recovers the original attempt
The browser SHALL retain immutable current-screen attempt UUID/canonical WAV after a possible upload admission. Explicit recovery SHALL read that original receipt, or reupload its exact UUID/bytes if no receipt is found. It SHALL NOT allocate a second attempt or delete the first as ordinary transport retry.

#### Scenario: Lost upload acknowledgement or transient failed poll
- **WHEN** an upload may have committed but its acknowledgement or later status request is lost
- **THEN** explicit result recovery uses the original identity and can deliver the original result without another bill.
- **WHEN** that attempt is returned as unknown/cancelled/expired
- **THEN** it remains closed with honest status and no automatic resubmission.
