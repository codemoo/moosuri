## ADDED Requirements

### Requirement: Bounded inert text intake
The worker SHALL accept only same-origin exact navigation POST /share-target with
multipart title/text/url, bounded to32KiB and8s before parsing and8192 combined UTF-8
bytes. Files, duplicate and unknown parts SHALL fail. HTML and URLs SHALL stay inert.

#### Scenario: Malicious multipart or private URL
- **WHEN** incoming multipart exceeds bounds or has file/duplicate/unknown fields
- **THEN** no payload is staged and only a generic no-store redirect is returned
- **AND** valid HTML/URL text is never executed, fetched or put in a query/cache

### Requirement: Authenticated intended-owner handoff
A separate temporary plaintext store SHALL retain at most one payload for5min absolute
TTL, with random ID/device/generation/intended owner/original scope. Fresh current
session and exact intended account SHALL precede disclosure and insertion. A failed
GET SHALL reveal nothing. Public shell Cache API SHALL remain public only.

#### Scenario: Session expiry and login
- **WHEN** a known owner's ordinary session expires before import
- **THEN** the pending payload remains opaque until explicit same-account import with fresh supported scope
- **AND** a different account destroys it, while an unbound401 intake requires login then re-share

#### Scenario: Remote revocation versus natural expiry
- **WHEN** a same-owner renewed login tries to recover a handoff received before or after ordinary expiry
- **THEN** owner-authenticated bounded POST validation must establish the retained original unrestricted current-epoch session row
- **AND** individual logout/revoke, password reset/change, suspend/resume, recovery/epoch transition, missing/null/corrupt origin and failed proof never disclose plaintext
- **AND** the origin namespace is private POST data and never authentication or an admin bypass

#### Scenario: Same-account cookie replacement between session GET and final proof
- **WHEN** an existing claim expects session A but the final origin POST authenticates a fresh same-account cookie B
- **THEN** mandatory expected_current_scope must reject the proof before disclosure or insertion
- **AND** an unclaimed origin A explicitly imported as a new B claim can still validate origin A with expected-current B

#### Scenario: Delayed transaction completion after another context retires
- **WHEN** claim or consume commits but its owning window receives completion after another context clears durable authority
- **THEN** fresh server session/origin proof and a final durable-generation transaction reject disclosure and insertion without a broadcast hint
- **AND** the UI write runs synchronously inside that final transaction and a retirement notification hides already displayed review

#### Scenario: Logout races worker intake
- **WHEN** explicit logout/password or observed cross-tab auth change retires a lifetime while an older operation is pending
- **THEN** synchronous local fence and durable generation CAS reject late bind/stage/claim/consume
- **AND** a retired scope cannot restage while the logout cookie request is pending

### Requirement: Single-window one-use review
Claim SHALL be atomic and exclusive to one window without automatic lease transfer.
Worker termination before claim SHALL preserve the committed handoff. Consume SHALL
erase payload and retain a bounded tombstone until original TTL before appending to
the latest original-slot draft. Claim or insertion SHALL never send a chat request.

#### Scenario: Replay and lost window
- **WHEN** two windows race, a claimed page reloads, or a consumed ID is replayed
- **THEN** at most one window receives plaintext and the URL cannot import it again
- **AND** page loss after claim requires re-share, with no automatic claim transfer

#### Scenario: Draft editing and changed thread
- **WHEN** the user edits the share and types in the ordinary draft or changes threads
- **THEN** explicit insertion appends to the latest original-slot text only within8192 UTF-8 bytes
- **AND** changed-thread insertion is blocked and normal Send alone submits chat

#### Scenario: Interruption at consume
- **WHEN** the page dies after payload erasure but before draft append
- **THEN** the handoff can be lost and the user must re-share, never automatic duplicate import
