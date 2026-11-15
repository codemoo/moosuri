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

#### Scenario: Output nouns do not authorize Calendar effects

- WHEN the owner requests a table of Calendar events, a meeting summary or an event report
- THEN the mission MAY perform supported reads and report work but SHALL admit no Calendar effect from that output-creation verb
- AND a supported event command SHALL retain its original exact create/date-move operation

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

#### Scenario: Pinned local Gitea login and issue count
- **WHEN** a current direct chat requests one exact repository already selected for Gitea1.24.6 on a trusted native device
- **THEN** the same original credential read job SHALL wait for an unlocked vault and confirmed unexpired local browser
- **AND** the local adapter SHALL check the selected item, exact origin/version/forms and TOTP challenge before using only that account's username/password/current code
- **AND** only the selected repository's bounded numeric open issue count and signed provenance SHALL reach the original chat; secrets/cookies/DOM/CDP/raw errors SHALL stay local
- **AND** a second matching request SHALL reuse the scoped binding without per-request approval while restart/revoke/unknown SHALL retain the existing no-replay and lifetime rules

#### Scenario: Unsupported or uncertain browser effect
- **WHEN** an arbitrary submission is unsupported or an entered action has an unknown outcome
- **THEN** the system SHALL retain a truthful unsupported/unknown result and no automatic replay
- **AND** operator-browser manual takeover SHALL not be advertised as native-vault secrecy

### Requirement: Explicit numeric report adoption qualifies only supported structure

The system SHALL qualify a fixed saved-source search/read → source-bound exact DAG → requested private report recipe only after the current owner adopts two independently finalized report artifacts. Numeric/factual postconditions SHALL NOT certify narrative quality; review_required and mission review status SHALL remain unchanged.

#### Scenario: Adoption revalidates original report evidence

- WHEN the current owner adopts a supported original report
- THEN proof SHALL revalidate actual search/read and arithmetic child receipts, current complete source identity, distinct normalized inputs and source identity
- AND artifact body adoption and exact current artifact revision SHALL remain valid

#### Scenario: Reuse does not manufacture training success

- GIVEN two adopted independent supported reports produce a usable owner-local pin
- WHEN a new accepted report mission invokes that pin with fresh literal parameters
- THEN the system performs fresh reads and exact child calculations within the existing twelve-tool budget and uses the current report writer for its output
- AND recipe replays, recurring occurrences, imported instruction examples and model success claims SHALL NOT become new proof

#### Scenario: Editing creates a new unproved revision

- WHEN the owner edits a supported recipe with its current revision
- THEN a new typed recipe revision SHALL start as candidate without inherited proof
- AND the previous pin SHALL be removed with a tombstone and actually dependent outputs/schedules SHALL be withdrawn or paused
- AND old adopted outputs SHALL NOT qualify the new revision through replay

#### Scenario: Artifact changes withdraw qualification

- WHEN a qualifying artifact is edited, deleted, withdrawn or changes current revision
- THEN its qualification proof SHALL be removed immediately and dependent uses SHALL be invalidated
- AND unrelated discovery-only pins SHALL NOT pause their schedules
- AND re-enabling an available pin SHALL NOT automatically resume a paused schedule

#### Scenario: Portable package contains structure and synthetic examples only

- WHEN an owner previews a supported compound recipe export
- THEN the package SHALL be rebuilt from the allowed typed structure and repository-owned synthetic examples
- AND it SHALL contain no private trace, source/account/credential bindings, original values, content or proof
- AND recipients SHALL use the existing Git/card lifecycle and begin without inherited owner-local qualification

### Requirement: Original local draft results qualify preparation without inheriting approval

The system SHALL admit a source search/read → source-bound exact calculation → local PrepareMail recipe only through dedicated current-owner adoption of the original D MailAction identity, revision and digest. Two distinct current inputs and sources SHALL be required. The recipe SHALL contain structure only and SHALL keep narrative/recipient review required.

#### Scenario: Draft adoption verifies source and arithmetic

- WHEN the current owner adopts an original draft result
- THEN the actual finalized draft body and result SHALL adopt the observed source title, numeric prefix, source number and independently recalculated exact result

#### Scenario: Fresh draft replay remains under original D authority

- GIVEN two independently finalized and owner-adopted local draft results qualify a private pin
- WHEN a newly delegated task explicitly requests draft preparation and supplies fresh source/value/recipient/body inputs
- THEN the recipe SHALL perform new actual reads and calculations within the existing twelve-tool budget
- AND the current D proposal SHALL prepare a new original local MailAction and retain its actual source receipt linkage
- AND no approval, send, provider draft save, Calendar effect or prior account/resource/credential binding SHALL be a replayable recipe node
- AND prior approval SHALL NOT authorize the new action

#### Scenario: Learned child source is resolved by actual address

- WHEN D receives an actual EvidenceCalculation receipt from a learned execution
- THEN it SHALL resolve only that receipt's actual same-run current-acceptance source address and verify current source identity/revision
- AND numeric equality or matching text SHALL NOT establish source identity

#### Scenario: Draft changes and unknown effects withdraw proof

- WHEN a qualifying original draft is edited, rejected, changes timing revision, or settles as failed/unknown
- THEN proof and actually used descendants SHALL be invalidated and only dependent schedules SHALL pause
- AND source/account withdrawal SHALL retain the existing run dependency revocation behavior
- AND unknown external outcomes SHALL NOT create another action, resend or new proof

#### Scenario: Draft recipe editing and sharing preserve owner isolation

- WHEN a draft recipe is edited
- THEN a new draft recipe revision SHALL start without inherited proof and old adoption route bindings SHALL remain immutable
- WHEN the owner exports it through the existing community preview
- THEN the package SHALL reconstruct only the typed structure with new synthetic examples and preserve the existing Git/card lifecycle
- AND the receiving account SHALL inherit no private content, recipient, binding, credential, proof or send authority


### Requirement: Local browser groundwork does not imply website authority

The trusted companion SHALL expose local-human-only fresh private browser sessions with inherited pipe transport and a bounded page-count/lifecycle projection. Model/server browser execution, secret use and site-content export SHALL remain unavailable before a separately reviewed site adapter.

#### Scenario: Recovery retains the original session boundary

- WHEN a local browser session is recovered after restart
- THEN its journal and original lifetime/idle/handoff limits SHALL survive without attaching to or signalling a retained process identity
- AND recovery SHALL require a new blank profile and local human confirmation

#### Scenario: Journal failure during local lock

- WHEN the user locally locks the companion and persistence fails
- THEN the unlocked vault SHALL already be retired and browser suspension SHALL not preserve it for subsequent polling
- AND the failure SHALL not become a successful cleanup claim
