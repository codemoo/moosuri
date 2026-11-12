## ADDED Requirements

### Requirement: Memory intent comes from the current authenticated request
The system SHALL construct memory planning input from only the current authenticated
user message and fixed instructions, validate a completed typed decision and literal
quotes, and never derive write authority from retrieved content or prior model output.

#### Scenario: Source contains memory-changing instructions
- **WHEN** retrieved text asks the assistant to save, change or delete a fact
- **THEN** the read-only tool branch cannot create a memory command slot

#### Scenario: Quoted or ambiguous natural language
- **WHEN** planning cannot identify a clear direct undated instruction and literal fact/target
- **THEN** it selects ordinary chat or clarification without mutation, while semantic classifier quality remains separately evaluated

### Requirement: A clear memory command has one durable local result
The system SHALL seal one stable command slot per run, resolve one current account-scoped
target with a pinned revision, and atomically record the local result and confirmation.

#### Scenario: Lost acknowledgement and explicit retry
- **WHEN** a sealed instruction is resumed after interruption or the provider changes its call ID
- **THEN** the same slot is reused without a replacement model plan or duplicated mutation

#### Scenario: Ambiguous or changed target
- **WHEN** a target is missing, ambiguous, truncated or changed after preparation
- **THEN** no memory mutation occurs and the user receives a fixed clarification

### Requirement: Adopted forgetting survives crashes without reviving conversations
The system SHALL guard and fsync an opaque forget intent before SQLite application,
retain journal-resolution metadata independently of chat deletion, and reconcile its receipt.

#### Scenario: Crash after journal append
- **WHEN** the adopted forget record exists but the database application did not commit
- **THEN** startup applies it once and records its result without a new model call or recreated deleted chat

#### Scenario: Crash before journal append
- **WHEN** only a prepared command row exists
- **THEN** startup does not treat it as success or execute it under expired authority

### Requirement: Mutation confirmation cannot preserve stale model content
The system SHALL unconditionally replace old assistant output, event replay, private
continuations and dependencies with only a receipt-derived terminal confirmation,
while preserving user text and content-free accounting.

#### Scenario: Correction invalidates its own context
- **WHEN** correcting a memory purges the command run's inherited knowledge context
- **THEN** the user still receives one truthful fixed confirmation and no stale derived text survives

#### Scenario: Planner returns commentary or no-command
- **WHEN** an intent model emits text/metadata previews or chooses ordinary chat
- **THEN** planning previews remain hidden and normal chat receives a fresh request without the classifier capsule, while its usage and budgets remain recorded

### Requirement: Targets follow effective-time search without weakening mutation CAS
The system SHALL resolve unique targets using current effective revisions, pin both
effective and head revisions, and recheck uniqueness before mutation. A future-dated
head SHALL NOT hide an older current match or silently replace the user's intended
current target.

#### Scenario: A future head hides one of two current matches
- **WHEN** two current memories match a forget request and one has a future correction
- **THEN** the system asks for a narrower target and changes neither memory

#### Scenario: Time or another write changes a sealed target
- **WHEN** the effective revision, head revision or uniqueness changes before mutation
- **THEN** the command completes with truthful clarification and no memory mutation

### Requirement: Retried read operations do not re-enter memory planning
The system SHALL retain run-wide memory-planning eligibility across attempts and
skip intent classification after a knowledge tool has already executed.

#### Scenario: Recall fails after reading memory
- **WHEN** a memory-cued recall has read a memory, its model continuation fails and the user retries
- **THEN** retry continues the read-only path without another intent-classification dispatch

### Requirement: Broad personal recall can inspect bounded current memories
The system SHALL allow an empty literal query only for memory browsing, return bounded
account-scoped effective revisions, and preserve the same cursor, generation, budget
and derived-answer revocation rules used by literal retrieval.

#### Scenario: A category question has no exact stored phrase
- **WHEN** a personal recall question does not contain a literal term from the saved fact
- **THEN** the agent can browse a bounded memory page and read the returned exact revision before answering

#### Scenario: Browsing encounters inactive or other-account facts
- **WHEN** current memory pages are requested
- **THEN** expired, forgotten, invalidated, future-only and other-account memories are excluded without reviving older expired revisions

#### Scenario: A result page is incomplete
- **WHEN** the returned page has a continuation cursor or is truncated
- **THEN** the agent can continue within the same run budget and SHALL NOT treat that page as proof that no other memory exists

### Requirement: Live classification evaluation reports its actual scope
The opt-in evaluator SHALL use explicitly authorized account-backed access, separate
model, parser, transport and evaluation-apparatus failures, and identify source
digests and unevaluated cases without recording credentials or private prompts.

#### Scenario: A completed answer omits the required decision
- **WHEN** the model completes successfully without its classification function
- **THEN** the case is an evaluated model-format failure and later independent cases continue

#### Scenario: A failed transport blocks remaining cases
- **WHEN** evaluation cannot continue after a transport or apparatus failure
- **THEN** blocked cases remain unevaluated and report finalization does not mark evaluation complete
