## ADDED Requirements

### Requirement: One durable runtime per user
The installation SHALL allocate at most one active runtime for each account and a distinct persistent volume. Each active account SHALL own its authentication, sessions, assistant state, connector credentials, and execution progress within its runtime.

#### Scenario: Restart while another member is active
- **WHEN** a member's container is replaced or the host restarts
- **THEN** that member resumes from their own volume with one writer and other members cannot read their state

#### Scenario: Recover configured active members after host restart
- **WHEN** an operator explicitly enables the Linux recovery worker for pinned account generations and the host restarts
- **THEN** only configured registry-ready members are reconciled through the guarded Kiri profile and reported ready after owner/generation and kernel peer-UID checks
- **AND** a missing mount or stale generation for one member does not prevent another valid member from recovering
- **AND** reserved or offline fenced accounts are not initialized, activated or restarted by that worker

#### Scenario: Readiness socket names another process
- **WHEN** a configured recovery socket is replaced by a process with another host UID
- **THEN** the worker sends no HTTP health request to that process and does not report the member ready

### Requirement: Shared model capacity
The installation SHALL supply read-only shared modules and model availability, and broker model calls using installation credentials while isolating user history, external actions, and provider sessions.

#### Scenario: Two personal containers call one Moosuri engine
- **WHEN** members A and B request inference through their respective private model sockets
- **THEN** the same installation engine selects the configured upstream adapter, attributes each call to its originating member and returns results through that member’s socket
- **AND** each container keeps its conversation, tool permissions and work state on its own volume and receives no upstream credential

#### Scenario: Call a login-backed upstream
- **WHEN** a member submits a text request to an engine configured for official ChatGPT or Claude Code login
- **THEN** the broker verifies its dedicated worker UID, account, tenant, volume, generation, model and connection revision before admitting a fresh request and sending the prompt over the same connection
- **AND** an unavailable worker leaves a fresh budget untouched and a duplicate request can still retrieve its original receipt
- **AND** the upstream credential and CLI authentication environment remain on the host

#### Scenario: Rotate the installation model credential
- **WHEN** two members use the same provider and its installation credential rotates
- **THEN** each retains their own conversation and neither member obtains the provider credential or the other's session

### Requirement: Independent hosted operation
The assistant SHALL support personal Google sign-in, mail lookup, and scheduled work on the deployment host without an always-on client device.

#### Scenario: Link mail from a phone
- **WHEN** a signed-in member grants Google access from a pending latest-mail request
- **THEN** the same member's runtime stores the grant and resumes the eligible read once while the member's computer is off

### Requirement: Fenced migration and recovery
The installation SHALL move each account with owner-specific evidence, forward-only handling of uncertain external effects, and a verified recovery procedure.

#### Scenario: Worker interrupted after external dispatch
- **WHEN** the runtime is recovered while a sent operation has an unknown result
- **THEN** the new owner reconciles the existing receipt and does not automatically send the operation again

### Requirement: Authoritative administration
The installation SHALL serialize account management decisions against current actor authority, target revision and usable administrator eligibility. A runtime SHALL apply the recorded command idempotently. A restored volume SHALL NOT grant an earlier privilege.

#### Scenario: An administrator is demoted while another command is pending
- **WHEN** an administrator retries a management request after demotion or two administrators concurrently demote one another
- **THEN** current installation authority is enforced, at least one usable administrator remains, and an unacknowledged local change remains pending

### Requirement: Route callbacks without granting authentication
The gateway SHALL route a provider callback only to its original purpose, account, and active generation. Completing consent SHALL require the original member session within that runtime.

#### Scenario: The active browser account changes during Google login
- **WHEN** member A starts Google consent and returns after switching the web app to B
- **THEN** neither B's session nor B's mail connection changes, and the callback alone does not resume A's request

### Requirement: Independently recoverable personal state
The migration SHALL establish an authenticated child backup baseline from a fenced parent checkpoint before enabling writes. Account deletion and vault revocation SHALL remain deliverable after the account runtime is removed.

#### Scenario: Source-free recovery after an interleaved deletion
- **WHEN** a migrated account is restored without the original server after both accounts have modified or deleted data
- **THEN** its receiver verifies the new lineage, deleted state stays deleted, no other member's records appear, and installation-wide spending cannot roll back


### Requirement: Explicitly scoped offline extraction
A partial extraction SHALL name its supported data scope, preserve the exact AccountId and TenantId with intended volume/generation metadata, and identify every excluded classified table. It SHALL NOT produce runnable authentication or claim cutover readiness before authenticated recovery lineage and complete reconciliation exist.

#### Scenario: Extract manual knowledge from a mixed-owner snapshot
- **WHEN** an operator rehearses the manual-knowledge scope using an explicit stopped synthetic snapshot and owner pair
- **THEN** the new private SQLite archive contains only that owner's retained knowledge graph, preserves evidence and deletion/temporal barriers, projects no authentication or role authority, rebuilds FTS without copying source shadow pages, and leaves the source unchanged

#### Scenario: Unsupported or ambiguous knowledge provenance
- **WHEN** the snapshot has an unknown schema, an invalid owner path, unsupported connected/automatic provenance, a foreign embedded evidence reference or deleted plaintext
- **THEN** extraction fails closed, reports only counts/fingerprints/reasons, preserves any existing destination, and publishes no valid archive

#### Scenario: Inspect the archive without the original source
- **WHEN** the original synthetic source is unavailable
- **THEN** verification can reconcile the archive's owner binding, exact retained rows, foreign keys, evidence, deletion state and rebuilt index while explicitly withholding runtime/cutover and authenticated-lineage claims
