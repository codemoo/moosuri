## ADDED Requirements

### Requirement: Outcome-bound composition reuses the durable harness
The system SHALL bind bounded task steps, input slots, actual receipt dependencies and requested outputs to the original owner, acceptance revision and capability profile. Existing run, tool, artifact and action services SHALL remain authoritative. Old text plans SHALL NOT acquire new effect authority through migration.

#### Scenario: Research produces one requested report
- **WHEN** an owner delegates supported source reading, calculation and private report creation
- **THEN** the actual report revision SHALL reference read evidence and completed calculation receipts
- **AND** restart or lost acknowledgement SHALL not create a second report or repeat a completed effect

#### Scenario: Qualitative uncertainty remains visible
- **WHEN** mechanical checks pass but a semantic acceptance condition is unresolved
- **THEN** the result SHALL retain review-required status and available work
- **AND** a model completion statement SHALL not certify the unresolved condition

### Requirement: Waiting preserves original identity and finite capacity
An intermediate wait SHALL release execution slots while retaining task, step, accepted revision, consumed budget and earliest applicable expiry. Unlock, answers and reconnect SHALL NOT renew authority or reset accounting. Initial admission SHALL allow two waiting plus one runnable/paused mission per owner, sixteen unfinished globally and one executing mission.

#### Scenario: Existing account resumes a valid task
- **WHEN** an exact supported binding becomes ready before the original deadlines
- **THEN** the same unfinished step SHALL resume with current authority and a fresh lease
- **AND** repeated readiness SHALL not create another task or dispatch

#### Scenario: Expired or superseded wait
- **WHEN** the wait expires, its accepted revision changes or its account/connection is revoked
- **THEN** a late answer SHALL not restart inference or create a new effect
- **AND** committed effects SHALL use original receipt reconciliation without replay

#### Scenario: Two ready waits share an occupied owner slot
- **WHEN** two waits become ready while another runnable or explicitly paused parent occupies the owner slot
- **THEN** readiness SHALL remain durable without over-admission or a lost wake-up
- **AND** the writer SHALL promote one valid ready parent in original admission order after slot release using the same direct/recurring/resume capacity rules
- **AND** explicit pause SHALL not be silently evicted and ready expiry SHALL be processed without a page visit

### Requirement: Connection setup fills only an admitted unresolved slot
A missing binding SHALL be represented at admission by a typed slot for an implemented/versioned operation and actually probed model/tool profile, bounded to requested operations, resources and account selection. Setup SHALL fill only that slot under current authority checks. It SHALL NOT refresh the whole profile or add a tool, schema or permission.

#### Scenario: Setup cannot upgrade an old run
- **WHEN** connection setup completes for a run without that supported unresolved slot
- **THEN** the run SHALL remain unavailable for the new capability
- **AND** a fresh explicit request SHALL be required while valid existing slots need no repeated routine approval

### Requirement: Credential selection and content use remain distinct
Valid scoped bindings SHALL be reused automatically; first use or genuine ambiguity SHALL require only the missing selection. Native model-readable mail SHALL require a separate grant, operation/profile and provenance path. Existing display-only results SHALL remain excluded from model context, recall, reports and learning. Credentials SHALL remain on the trusted device.

#### Scenario: Model-readable mailbox search
- **WHEN** the owner grants a supported native mailbox read scope and requests a matching message/thread
- **THEN** only bounded selected content and authenticated provenance SHALL reach the original task
- **AND** token/account mismatch or unsupported attachment formats SHALL be reported truthfully

#### Scenario: A display-only grant is insufficient
- **WHEN** only a native preview/body display grant or stored display record exists
- **THEN** a composed task SHALL not turn it into model-readable evidence
- **AND** reads within a separately valid model-read grant SHALL not require routine reapproval

### Requirement: Composed actions preserve domain approval and receipts
Tasks SHALL link existing domain actions by stable identity, revision and payload digest. Every mail send SHALL require final-preview approval of exact current account, recipients, content and timing. Model inference expiry SHALL not be renewed by later approval. Calendar SHALL retain explicit current-owner primary solo-event and ETag constraints.

#### Scenario: Draft preparation and later send
- **WHEN** a task prepares a draft and the owner later approves its exact final preview
- **THEN** only the existing approved action SHALL dispatch, with mission status following its real receipt
- **AND** content edits SHALL invalidate approval and unknown outcomes SHALL not trigger resend or device failover

#### Scenario: Evidence supplies values but no new operation
- **WHEN** a task uses read source values for a requested supported Calendar action
- **THEN** values SHALL retain evidence and target checks while authority comes from the owner request
- **AND** source instructions or ambiguous targets SHALL not independently cause a write

### Requirement: Compound learning requires supported independent evidence
A recipe SHALL contain supported typed structure, fresh input slots, receipt edges and checks, bounded to eight nodes/nine tool kinds within existing budgets. Two distinct adopted finalized outcomes SHALL qualify supported objective structures. Semantic claims, imported samples and recipe reuse SHALL not self-certify success. Send approval SHALL never be a replayable node.

#### Scenario: Fresh reuse and owner editing
- **WHEN** independent A/B tasks qualify a structure and the owner requests fresh C
- **THEN** execution SHALL read current inputs/sources and record new receipts without A/B data or bindings
- **AND** edits SHALL create a new revision without inherited qualification

#### Scenario: Sharing and withdrawal
- **WHEN** the owner exports a supported recipe
- **THEN** it SHALL use structural data and new synthetic examples without private traces or credentials
- **AND** recipients SHALL qualify with their own accounts/proofs; withdrawal SHALL preserve existing dependent-result and schedule rules

### Requirement: Native browser automation preserves the device boundary
Native-vault password/TOTP operations SHALL execute only through a supported adapter in a trusted local browser and broker. Secrets, cookies and unrestricted browser control SHALL not reach the model or operator browser. Each adapter SHALL fix its site, allowed task, observations, original session lifetime and outcome evidence before advertising support.

#### Scenario: Authentication handoff
- **WHEN** login, MFA or a device gesture is required
- **THEN** the task SHALL wait at the supported local handoff without exposing credentials or extending session life
- **AND** only a new valid observation SHALL resume supported work

#### Scenario: Unsupported or uncertain browser effect
- **WHEN** an arbitrary submission is unsupported or an entered action has an unknown outcome
- **THEN** the system SHALL retain a truthful unsupported/unknown result and no automatic replay
- **AND** operator-browser manual takeover SHALL not be advertised as native-vault secrecy
