## ADDED Requirements

### Requirement: Bounded transport recovery
The browser MUST bound an SSE connection attempt to 15 seconds and a pending raw-byte read to 45 seconds, preserving authenticated scope and never automatically resubmitting a model request.

#### Scenario: Socket stays open without heartbeats
- **WHEN** an opened stream receives no bytes for 45 seconds
- **THEN** the browser cancels it and resumes through authenticated GET reconciliation and its event cursor

#### Scenario: Quiet healthy conversation
- **WHEN** only valid comment heartbeats arrive every 15 seconds
- **THEN** the browser keeps the connection without requiring model output

#### Scenario: Owner signs out during a stalled transport
- **WHEN** the request scope closes before the transport settles
- **THEN** pending waits settle as aborted, timers are removed and no late body reaches the old account UI

### Requirement: Consecutive recovery budget
The client MUST stop after six consecutive unhealthy failed connections. New validated cursor progress or a decoded comment heartbeat after at least 15 seconds of connection lifetime MUST reset that budget; opening alone MUST NOT reset it.

#### Scenario: Recovered connection fails much later
- **WHEN** more than six disconnections are separated by validated event or delayed heartbeat delivery
- **THEN** automatic recovery remains available

#### Scenario: Server immediately closes each response
- **WHEN** six connections open and fail without validated progress or delayed heartbeat
- **THEN** automatic attempts stop and the existing manual history refresh remains available

### Requirement: Cursor-safe fresh submission receipt
A fresh accepted send MUST retain its live event cursor and loaded history when the same request is confirmed through the current stream within one second. Cursorless POST output MUST NOT merge with incremental events. Missing receipts and explicit same-ID resubmission MUST retain canonical snapshot reconciliation.

#### Scenario: POST acknowledgement races streamed answer
- **WHEN** a fresh POST succeeds and the current stream confirms its request ID
- **THEN** the answer is rendered once, older loaded pages stay visible and no extra history GET or stream replacement is required

#### Scenario: Receipt never arrives
- **WHEN** one second passes without confirming the accepted fresh request
- **THEN** the browser reloads a canonical page, resumes its exact cursor and does not repeat POST

#### Scenario: Logout while awaiting receipt
- **WHEN** the account closes before receipt timeout
- **THEN** delayed reconciliation cannot restore old private messages or initiate an old-account history GET

### Requirement: Bounded read-only snapshot requests
Read-only JSON GET requests MUST use one 30-second total deadline covering fetch headers and the complete response body. Deadline expiry MUST surface as an ordinary failure, cancel transport and release loading; account closure MUST promptly settle as aborted even when transport ignores AbortSignal. Mutation timing and dispatch semantics MUST remain unchanged.

#### Scenario: Snapshot headers or body stall
- **WHEN** a canonical snapshot GET has not completed within 30 seconds from request start
- **THEN** it fails visibly, leaves loading and permits explicit GET-only refresh without replaying POST

#### Scenario: Stalled snapshot returns after recovery
- **WHEN** an expired GET later returns its response headers or bytes
- **THEN** its body is cancelled and its stale result cannot replace the current owner-scoped conversation

#### Scenario: Account closes while GET is stalled
- **WHEN** account closure aborts a request whose fetch or body ignores AbortSignal
- **THEN** the pending GET promptly settles as aborted and clears its deadline without exposing old private output

#### Scenario: Mutation remains pending beyond GET deadline
- **WHEN** a POST acknowledgement is still pending after 30 seconds
- **THEN** the read-only deadline does not alter or replay that mutation
