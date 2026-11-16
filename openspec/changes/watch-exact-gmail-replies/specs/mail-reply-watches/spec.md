## ADDED Requirements

### Requirement: Receipt-scoped truthful reply monitoring
A reply watch MUST bind one owned server-managed Gmail conversation observed in a
completed foreground read or confirmed send. It MUST NOT invent provider IDs,
claim monitoring from a mere read, broaden collection scope or authorize sending.

#### Scenario: Direct request has one known target
- **WHEN** the user asks to be notified of a reply in the original conversation
- **THEN** a durable watch is registered and its actual status is shown
- **AND** missing or multiple targets require target selection without a false registration claim

#### Scenario: Waiting is conversational rather than a request
- **WHEN** the user says they will wait and no watch exists
- **THEN** the assistant proposes the available control without claiming it is already running

### Requirement: Metadata-first detection with no unchanged model cost
An unchanged check MUST use only bounded provider metadata and MUST NOT admit a
model run, read old message bodies or publish a reply notification. Partial or
failed checks MUST remain distinguishable from an unchanged successful check.

#### Scenario: Reply precedes the first poll
- **WHEN** an inbound message arrives after the actual observed baseline and before registration or first polling
- **THEN** it is detected and read rather than silently becoming the baseline

#### Scenario: Labels or outbound messages change
- **WHEN** an existing message becomes unread or a new SENT/DRAFT message appears
- **THEN** no reply notice or model call is produced

#### Scenario: New inbound content exists
- **WHEN** up to four unseen inbound messages follow the observed baseline
- **THEN** only those bodies are read and bounded literal previews are stored in the original chat
- **AND** one inbox event is published and the next-reply watch completes

### Requirement: Durable bounded authority and delivery
Watch leases, observation cursors and notifications MUST preserve owner isolation,
idempotency and current account/connection/app authority through publication and
passive Push display. An uncertain body or failed save MUST NOT advance the cursor.

#### Scenario: Stop races a provider response
- **WHEN** the user stops the watch or disconnects Gmail while a body read is in flight
- **THEN** its late result cannot publish a preview or a new notification

#### Scenario: Process restarts or a request response is lost
- **WHEN** the service restarts after a claimed read or the create response is lost
- **THEN** the lease is reclaimed after its deadline and the same create receipt remains one watch
- **AND** successful notification and reply-state persistence commit or roll back together

#### Scenario: Session ends or restored credentials are quarantined
- **WHEN** an ordinary session logs out
- **THEN** the explicit owner grant can keep checking until its original expiry
- **WHEN** data is restored or the account epoch changes
- **THEN** the old watch is withdrawn and cannot resume from restored credentials
