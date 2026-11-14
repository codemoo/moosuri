## ADDED Requirements

### Requirement: Explicit supported human request evidence
The producer SHALL read only a user-selected directory and explicitly supported
public Codex JSONL subset with root CLI/user session and user-triggered root turn
attribution. Only literal bounded prose user_message text SHALL become evidence.
Automation, subagents, inherited/unknown formats, tool/assistant/config and
attachment records SHALL NOT be request evidence or executable instructions.

#### Scenario: A mixed human and automation directory
- **WHEN** a selected directory contains proven human requests, exec/subagent logs, context, tools, assistant text and credential-looking user content
- **THEN** only selected human prose is staged and unsupported/deferred counters remain honest
- **AND** no tool, model or source autodiscovery runs

### Requirement: Stable bounded local file and turn cursor
The producer SHALL use bounded once-only scanning, no-follow regular files,
inode/stat/hash stability checks and semantic turn dedup. Incomplete lines SHALL
not be parsed. Finished markers SHALL not attest work success; missing/failed
completion SHALL remain partial, with an idle delay for active turns. Absent
files SHALL not delete remote evidence.

#### Scenario: Partial append and inode replacement
- **WHEN** an active partial file later gains a complete matching lifecycle record or is atomically replaced
- **THEN** only complete request evidence updates the same turn identity and unchanged semantic hashes do not duplicate it

### Requirement: Exact destination and durable uncertain batch recovery
The producer SHALL pin the expected connection/exact HTTPS origin, private token/state
and root/current-UID ancestors (nonsticky mutable refused; sticky needs next owned
child). It SHALL persist exact request/body/run/cursor before send and never log/store
credentials. Cursor advances after receipt+finish or exact receipt/current after-position
with no active run. Unknown ACK reuses identity; auth/config/position/run mismatches
SHALL retain state and fail.

#### Scenario: Batch or finish ACK is lost
- **WHEN** source text changes after an unconfirmed batch or finish
- **THEN** rerun uses the original receipt/body/UUID and leaves the prior cursor until confirmation
- **AND** the changed text cannot replace an old pending request

#### Scenario: Redirect, wrong owner connection or unsafe private path
- **WHEN** status redirects, the token identifies a different expected connection, or token/state has unsafe ownership/link/location/type
- **THEN** no batch or cross-origin credential dispatch occurs and only fixed errors are reported

#### Scenario: Server restart fences a committed run unknown
- **WHEN** the actual Store/router retains a batch receipt but startup marks its unfinished run unknown
- **THEN** the producer reconciles only the exact original receipt/current after-sequence/checkpoint with no active run
- **AND** it advances the local cursor without duplicate ingest or rewriting unknown history

### Requirement: Honest bounded coverage and start uncertainty
Receipts SHALL distinguish partial producer reports from server proof or actual
work success. Missing run-start ACK SHALL retain the intent and stop without
adopting an unproven active run or allocating another request. Unsupported clients,
real Store/server/account verification and broad history coverage SHALL not be
claimed by mock protocol tests.

#### Scenario: Run-start result cannot be established
- **WHEN** an issued run-start request has no confirmed original run identity
- **THEN** later invocations stop with retained state for explicit reconciliation
- **AND** no automatic replacement request or source cursor advance occurs
