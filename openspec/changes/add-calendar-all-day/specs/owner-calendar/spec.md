## ADDED Requirements

### Requirement: Date-only primary solo event lifecycle
The service SHALL admit bounded primary owner solo nonrecurring all-day reads and explicit create/update/delete through the existing typed Calendar effect lifecycle. API start/end SHALL be strict civil dates with an exclusive end and null timezone. Malformed, empty, reversed, mixed or over31-day mutation ranges SHALL fail before dispatch. Event kind conversion SHALL be rejected.

#### Scenario: All-day create, move and delete
- **WHEN** the current owner creates March14 through March16 inclusively and later moves that exact event to November7
- **THEN** the provider receives date-only March14/March17 and November7/November10 boundaries
- **AND** actual receipts, unchanged target/ETag checks and exact If-Match remain authoritative across DST
- **AND** an explicit delete uses the observed event ID and ETag exactly once.

#### Scenario: Unknown or stale mutation
- **WHEN** an all-day mutation loses its response or the current ETag differs
- **THEN** the original action UUID is retained with lookup-only unknown state or definite conflict
- **AND** retry/lookup never resends an uncertain effect or proves a mismatched date shape.

### Requirement: Human date entry and grounded continuation
Forms SHALL show inclusive start and last day and keep date strings stable across device timezone/DST. Current-request commands SHALL ground exact supplied civil-date quotes, preserve frozen fields during bounded clarification, and preserve the observed day count when moving an all-day event. Timed behavior SHALL retain its existing clock/duration contract.

#### Scenario: Missing all-day last date
- **WHEN** an explicit several-day request omits its last day and the next bounded answer supplies it
- **THEN** only that missing date is completed; the original kind, title, start, owner/session/model and Calendar pins remain frozen
- **AND** general history/model text cannot supply a different target or conversion authority.
