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

#### Scenario: Answer a follow-up about a supplied fact
- **WHEN** the member asks for a value from a prior completed personal text turn
- **THEN** the personal service supplies the eligible same-thread history and instructs the selected model to preserve the provided value, use the requested format and acknowledge missing information.

#### Scenario: The selected model reaches its output limit before visible text
- **WHEN** the verified terminal receipt for the original call reports an incomplete outcome with no visible answer
- **THEN** the member sees the incomplete status on reload and the service does not repeat the call or expose model reasoning.

#### Scenario: Prior turns exceed a member's request allowance
- **WHEN** eligible conversation history exceeds the private socket frame or the per-call reservation reported by the installation broker
- **THEN** the personal service preserves the current question and newest complete exchanges that fit, while the member's stored conversation retains all its turns.
- **AND** an explicitly saved follow-up keeps its original question and fits an available prefix of the original answer. If its required input alone exceeds the allowance, it remains available without another model call.

#### Scenario: Two personal containers call one Moosuri engine
- **WHEN** members A and B request inference through their respective private model sockets
- **THEN** the same installation engine selects the configured upstream adapter, attributes each call to its originating member and returns results through that member’s socket
- **AND** each container keeps its conversation, tool permissions and work state on its own volume and receives no upstream credential

#### Scenario: Call a login-backed upstream
- **WHEN** a member submits a text request to an engine configured for official ChatGPT or Claude Code login
- **THEN** the broker verifies its dedicated worker UID, account, tenant, volume, generation, model and connection revision before admitting a fresh request and sending the prompt over the same connection
- **AND** an unavailable worker leaves a fresh budget untouched and a duplicate request can still retrieve its original receipt
- **AND** the upstream credential and CLI authentication environment remain on the host

#### Scenario: Administrator selects the installation model and reasoning effort
- **WHEN** a current installation administrator selects an available model and reasoning effort or selects the host default
- **THEN** newly admitted model calls from every bound member and workload use the same installation choice
- **AND** the broker checks the effective pair and the current account under the same lock as dispatch admission
- **AND** a paused engine admits no new inference and stale administrator edits are rejected
- **AND** prior member choices have no effect on the current installation and the gateway accepts only installation-wide settings
- **AND** a member cannot choose a different pair through the web or the model socket
- **AND** an upstream response that explicitly reports a different model ID cannot supply a saved or displayed answer, and retrying that same call only retrieves its accounting receipt

#### Scenario: An administrator checks the installation API key's provider-reported capacity
- **WHEN** a current administrator requests capacity for the enabled OpenRouter API connection
- **THEN** the broker reads official key metadata and the gateway returns scoped metrics with the observation time only to that administrator
- **AND** a reported zero remains zero, missing values remain unknown, and a reset period alone does not establish an instant
- **AND** a changed connection, revoked administrator authority, expired request or failed refresh cannot deliver earlier capacity as the new result

#### Scenario: Rotate the installation model credential
- **WHEN** two members use the same provider and its installation credential rotates
- **THEN** each retains their own conversation and neither member obtains the provider credential or the other's session

### Requirement: Independent hosted operation
The assistant SHALL support personal Google sign-in, mail lookup, and scheduled work on the deployment host without an always-on client device.

#### Scenario: Link mail from a phone
- **WHEN** a signed-in member grants Google access from a pending latest-mail request
- **THEN** the same member's runtime stores the grant and resumes the eligible read once while the member's computer is off

#### Scenario: Register GWS independently for two members
- **WHEN** two allocated members connect different Google accounts to their respective runtimes
- **THEN** each member runtime conducts its own OAuth exchange and refresh, and invokes the local GWS executable for the selected mailbox
- **AND** the installation relay forwards TLS to approved Google destinations using the active allocation and kernel peer identity without receiving OAuth tokens as application data

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

### Requirement: Personal service connection status
The personal connector catalog SHALL show the current member's service account and connection status. Searching, inspecting a connector, or completing a connection started from the catalog SHALL NOT read inbox contents.

#### Scenario: Inspect a linked Gmail account
- **WHEN** a member opens the connected list and selects Gmail
- **THEN** the mailbox identity and connection controls are available, and opening the screen does not dispatch a mail read

#### Scenario: A member connects a second Google Workspace service
- **WHEN** a member with an existing Gmail grant selects Drive, Docs, Slides or Sheets
- **THEN** each connector requests its own read scope and records the verified Google account in the member runtime; disconnecting one leaves the others available
- **AND** the authorization callback belongs to the original service, account, generation and session, and completing consent does not read a file

#### Scenario: Vault site configured before Kiri session integration
- **WHEN** the member opens the vault connector and a separate HTTPS vault origin is configured
- **THEN** the UI identifies the missing member-session integration and may open the configured site without carrying session credentials

#### Scenario: An enrolled member opens the vault and repeats a selective import
- **WHEN** an enrolled member opens the vault from a full member session
- **THEN** the vault accepts a short-lived single-use handoff bound to the member and current allocation, and the client unlocks the vault on its own origin
- **AND** the client can preview and select Apple Passwords CSV rows by username, preserve the selected OTPAuth data, and identify committed entries as duplicates on a repeat import
- **AND** logout or a change of allocation prevents later authorized writes under the old grant

#### Scenario: A member explicitly asks to sign in to Google
- **WHEN** the enrolled member requests Google sign-in in chat
- **THEN** the original thread records one attempt in a fresh Kiri Chrome profile and reports the provider outcome
- **AND** only verification of the requested account on the official Google account home can mark it signed in; a browser rejection, a human step or an uncertain response remains visible in the original task

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

### Requirement: Member-owned browser work in personal chat
The personal runtime SHALL bind browser work to the authenticated member, runtime generation, conversation and original task. It SHALL expose a human viewer separately from model tools, stop model observation and input during takeover, and resume only from a confirmed checkpoint.

#### Scenario: Inspect and dismiss a browser card
- **WHEN** a member opens the preview of an active task and then dismisses the viewer
- **THEN** the desktop side panel or narrow-screen full viewer closes, frame delivery ceases and the task remains accessible from the same chat

#### Scenario: Change a filter and return control
- **WHEN** the member changes a filter on the active page and confirms completion twice after a lost response
- **THEN** the original task observes the current page once for that accepted continuation and returns the filtered result without creating another task or replaying input

#### Scenario: Concurrent sessions and worker loss
- **WHEN** another member requests the frame, a superseded controller submits input, or Chromium exits after a possible submission
- **THEN** foreign access and stale input are rejected, ambiguous effects remain unresolved, and reconnecting does not replay the action

### Requirement: Historical browser tasks in the member workspace

The Kiri member workspace SHALL list the current member's browser tasks newest first, with a stable detail link for each original chat run. The detail SHALL show the request, next actor, current state, recorded transitions, saved observations or answer, and a link to the originating conversation. State history SHALL store only the state and available timestamp with its owning run.

#### Scenario: Review a task after the browser closes

- **WHEN** the member opens an older task after starting another task in the same conversation or restarting the personal service
- **THEN** the member can read the saved answer, distinguish it from a preliminary observation and return to the original conversation.

#### Scenario: Pick up a task waiting for the member

- **WHEN** the browser waits for input or hands control to the member
- **THEN** the task detail identifies the member's next step and follows later state changes while open.

#### Scenario: Review a task recorded before transition history existed

- **WHEN** a member opens an older task after the history migration
- **THEN** the detail retains its request and available output, shows the recorded start time and labels an undated historical state without inventing a timestamp.

#### Scenario: An old link outlives access to the task

- **WHEN** a member opens a link belonging to another account or a deleted conversation
- **THEN** the personal service returns no task, timeline or answer.

### Requirement: Installation-managed Google Workspace setup
The installation SHALL expose web OAuth client registration only to the current installation administrator and SHALL distribute validated app configuration to eligible member runtimes. Google login, account choices and personal grants SHALL remain separate from assistant inference.

#### Scenario: Prepare a project and resume
- **WHEN** an administrator runs the Google Cloud preparation command again using the same project ID
- **THEN** the command reuses the active project and verifies the five required Workspace APIs before reporting readiness.

#### Scenario: Open setup from a connector
- **WHEN** an installation administrator opens the connector list or a supported Google service
- **THEN** the administrator can open installation setup there and return to the same list or service. An ordinary member sees only account connection controls.

#### Scenario: Import an existing web client
- **WHEN** the administrator submits a web client for the exact installation callback
- **THEN** the gateway stores it privately, accepts an equivalent retry, rejects a conflicting registration and returns only setup status and the callback.

#### Scenario: Existing and newly provisioned members
- **WHEN** an eligible active member joins the managed relay before or after installation setup
- **THEN** the member can start their own Google consent without operator registration commands. A different manually configured client and its existing grants are preserved.


### Requirement: Member workspace navigation

The Kiri member workspace SHALL support browser back, forward and reload for the main views, connector detail, installation setup and engine settings. Navigation SHALL use screen and service names and opaque member task identifiers. Session authority SHALL govern every administration screen.

#### Scenario: Returning from installation setup

- **WHEN** an administrator opens Gmail, opens installation setup and goes back in the browser
- **THEN** Gmail detail is visible again, and another back returns to the catalog without starting a Google authorization or inbox read.

#### Scenario: Reopening an administrator destination

- **WHEN** a member opens a saved engine settings URL
- **THEN** an installation administrator can inspect the configured engine, while an ordinary member returns to chat without requesting administrator data.

### Requirement: Save a follow-up during a personal text answer
A member SHALL be able to attach a follow-up to an active personal text turn and see its receipt after reopening the conversation. Saving SHALL leave the original model request and its answer unchanged. After that turn ends the member MAY send the saved text as a separate text-only turn with the original request and available completed answer in context.

#### Scenario: An acknowledgment is lost or a member returns later
- **WHEN** saving or sending the same follow-up is retried with its original identifier and text
- **THEN** the member sees the existing receipt or follow-up turn, and the broker receives at most one model call for that follow-up.

#### Scenario: The original answer ends before a follow-up can be saved
- **WHEN** the server receives a new follow-up for a turn that has already ended
- **THEN** the server declines it and the member can recover the text in the composer without dispatching it.

#### Scenario: The original provider outcome is unknown
- **WHEN** the member explicitly sends a previously saved follow-up after an unknown original outcome
- **THEN** the new text-only request includes the original question and the follow-up, and does not repeat the original call or initiate a tool action.

### Requirement: Member task execution and recurring local research

The member workspace SHALL show dated work receipts for text, browser, thread-bound inbox reads and local research. Each receipt SHALL identify the original request and recorded stages. An authorized member MAY run a bounded search of effective memories or completed personal text conversations once or on a recurring elapsed-time interval. The member SHALL resolve source evidence against current access whenever it is displayed.

#### Scenario: Reopen research from a conversation

- **WHEN** a member asks for an explicit search of their own memories or previous text conversations
- **THEN** one task and one original chat turn identify the same request after a retry. The dated list distinguishes chat, manual and scheduled research, previews the first source still available, and opens its original request and individual evidence steps.

#### Scenario: Run independently of an open browser

- **WHEN** the installation admits an active member’s due schedule, including after an offline interval
- **THEN** the member checks the gateway's Unix peer identity and the pinned epochs and generation, records one occurrence for the latest due slot, and advances its next due time in the same transaction. Its detail shows both the due slot and the actual execution time.

#### Scenario: Reopen the history of one recurring assignment

- **WHEN** a member opens execution history from a schedule or from one of its retained occurrences
- **THEN** the dated, paginated history uses the original schedule identity and current member volume. It includes its manual and scheduled occurrences after retirement. A new schedule with the same title starts with an empty history.

#### Scenario: Withdraw a source or a grant

- **WHEN** a source is forgotten, a schedule is retired, a grant epoch changes, or a restore completes
- **THEN** historical task details respect current source visibility, retired schedules keep their earlier receipts, and automatic execution requires a current active grant.

#### Scenario: Notify when a public price meets the member's limit

- **WHEN** a member gives one public page and an inclusive or exclusive upper price in Korean won
- **THEN** the original conversation shows the limit and the check history. A verified price satisfying the limit generates one notice, including on the first read. It can generate another after a verified price outside the limit. A change in surrounding text, a missing price or an ambiguous price generates no notice. An unreadable observation preserves the last verified condition.
