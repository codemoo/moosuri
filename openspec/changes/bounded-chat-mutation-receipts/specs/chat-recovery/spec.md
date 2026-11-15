## ADDED Requirements

### Requirement: Bounded foreground mutation receipts
Foreground message admission, new-thread creation and run cancel/retry JSON requests MUST have one 30-second total deadline spanning headers and complete body. Deadline expiry MUST represent an unknown receipt and MUST NOT automatically replay a mutation. Caller/account cancellation MUST settle promptly and discard late output even when transport ignores abort. Other mutation/probe timing MUST remain unchanged unless explicitly opted in.

#### Scenario: Message receipt stalls
- **WHEN** message admission headers or body have not completed after 30 seconds
- **THEN** the client releases the sending lock, checks canonical history by GET, and preserves the original uncertain request UUID and immutable payload for explicit resubmission alongside the next draft

#### Scenario: Canonical admission precedes a stalled HTTP receipt
- **WHEN** authenticated history or SSE confirms the exact pending request ID
- **THEN** the pending request and redundant HTTP transport are released without resurrecting uncertainty, and a next draft can send after the admitted run completes

#### Scenario: Run action receipt stalls after admission
- **WHEN** a cancel or retry was admitted but its HTTP receipt stalls
- **THEN** expiry releases its action lock and refreshes canonical run state without repeating the action or altering expected_attempt

#### Scenario: New-thread creation receipt is unknown
- **WHEN** thread creation times out before its identity is confirmed
- **THEN** the client refreshes the authenticated thread inventory, retains the original immutable creation/message identities beside the next draft, and automatically posts neither another thread nor its message

#### Scenario: Owner closes a stalled foreground mutation
- **WHEN** the account closes or caller cancels before its deadline
- **THEN** the pending wait settles as aborted, its timer is removed, and late headers/body cannot update that conversation

### Requirement: Immutable thread creation admission
POST /threads MUST accept an optional request_id UUID. Reuse of the same account, UUID and original canonical title MUST resolve to the same created thread, including concurrent requests. Changed original titles MUST conflict; deletion MUST preserve a receipt that returns not_found instead of recreating the thread. Omitted request IDs MUST retain old-client behavior. Every call MUST enforce its current account/session boundary.

#### Scenario: Lost creation receipt and explicit retry
- **WHEN** thread creation succeeds but its response is lost and the user explicitly retries the same creation UUID and original title
- **THEN** the server returns the same thread and no additional conversation is created

#### Scenario: Changed creation payload or deleted thread
- **WHEN** a reused creation UUID carries a different original canonical title or its admitted thread was deleted
- **THEN** the request fails with conflict 409 or not_found 404 respectively and never creates a replacement thread

#### Scenario: Reload between creation admission and message admission
- **WHEN** an uncertain composer-first creation is recovered under its exact authenticated tab-session scope
- **THEN** no POST is automatic, and explicit retry first reuses the original creation UUID/title and then the immutable message UUID/payload while preserving the separate next draft

#### Scenario: Empty button recovers a creation
- **WHEN** the user retries an uncertain creation through the New conversation button
- **THEN** it reuses the same creation UUID/title, migrates any pending new message/draft to that thread and does not send the message without a separate explicit message action

#### Scenario: User selects another thread during creation
- **WHEN** a creation receipt arrives after the user switched threads and wrote a draft there
- **THEN** its new-thread draft/pending state migrates atomically without changing the selected thread or overwriting its draft

#### Scenario: Admitted thread was edited before creation recovery
- **WHEN** inventory exposed the admitted thread and it now has another draft or unresolved message at explicit creation recovery
- **THEN** recovery retains both original slots and request identities, makes no message POST and shows a conflict to finish that target before retrying the same creation identity
