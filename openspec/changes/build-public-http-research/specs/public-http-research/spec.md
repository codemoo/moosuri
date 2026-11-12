## ADDED Requirements

### Requirement: Public HTTP tools use actual bounded evidence
The system SHALL fetch actual public HTTP(S) page bytes and configured search hits,
with bounded output and explicit incomplete coverage; it SHALL NOT claim browser
or JavaScript execution, authenticated sessions, or fabricated fallback results.

#### Scenario: Search is unconfigured
- **WHEN** the installation has no search key
- **THEN** the admitted profile excludes web_search and a forged search is unavailable

#### Scenario: Unsafe destination or redirect
- **WHEN** an URL hop resolves to a private, local, reserved or metadata destination
- **THEN** the request is refused before connecting to that destination

### Requirement: Web IO preserves durable execution authority
The system SHALL persist typed web slots and validate tenant/session/run/provider,
current capabilities and existing budgets before and after asynchronous IO.

#### Scenario: Read is interrupted
- **WHEN** a started slot lacks a committed result
- **THEN** reuse reports unknown without automatically repeating HTTP

#### Scenario: Revocation during held read
- **WHEN** the session is revoked or owning content is deleted
- **THEN** late results cannot restore payloads or observed references

### Requirement: Observed web references come from completed tools
The system SHALL expose current-run observed URL/title/time/kind/truncation metadata
from successful persisted web results, preserving existing knowledge evidence.

#### Scenario: Source withdrawal purges dependent work
- **WHEN** existing dependency invalidation purges a run
- **THEN** its web result payloads and observed references disappear together

#### Scenario: Model invents a source
- **WHEN** a source URL appears only in model prose
- **THEN** it is not added to the observed-reference list

### Requirement: Web research grants no email authority
The system SHALL require final explicit human approval for every immutable email
send; public-page instructions and requests to send SHALL NOT replace that approval.

#### Scenario: Page requests a send
- **WHEN** public page text instructs the model to email content
- **THEN** no web tool approves or sends mail
