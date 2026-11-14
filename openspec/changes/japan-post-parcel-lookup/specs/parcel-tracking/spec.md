## ADDED Requirements

### Requirement: Fixed official latest-summary lookup
The system SHALL expose an available Japan Post typed parcel lookup only through an admitted current public-read profile with the exact new recipe pin. It SHALL build one fixed official HTTPS endpoint from a validated carrier/number, use bounded unauthenticated public HTTP and reject arbitrary hosts, redirects, unsupported carriers/formats, credentials or new action authority.

#### Scenario: Current known summary and not-found markup
- **WHEN** the official response matches the established English summary headers and exact requested tracking-number row
- **THEN** the result SHALL preserve raw latest date/status/office plus an actual source URL/retrieval time/visible excerpt.
- **WHEN** that row states the observed official not-found message
- **THEN** it SHALL return not_found with no invented parcel event.
- **WHEN** table or row shape is unrecognized
- **THEN** it SHALL return provider_changed with no successful status claim.

### Requirement: Literal input and same-thread typed receipt reuse
Store SHALL prioritize an explicit current Japan Post number, otherwise reuse only a unique actual typed receipt from the same account/thread within32turns. Store SHALL stamp current and original request/run provenance. Source/model text SHALL NOT supply authority-bearing IDs or another carrier. Ambiguous, conflicting or missing identity SHALL require clarification without dispatching an old number.

#### Scenario: Follow-up reuses number but refreshes observation
- **WHEN** the same owner asks where that parcel is after its actual lookup
- **THEN** a null tracking number SHALL resolve to the exact prior typed number and perform a new official read
- **AND** actual new not-found/provider-changed states SHALL remain distinguishable from the earlier status.
- **WHEN** the same words arrive in another thread without its own receipt
- **THEN** no lookup SHALL be sent using a different thread's number.

#### Scenario: A new current number overrides prior identity
- **WHEN** a prior actual lookup used A and the current owner explicitly supplies Japan Post number B but the model omits its number argument
- **THEN** Store SHALL query B, never A, and stamp the current request as B’s number source.

#### Scenario: Conflicting current numbers cannot reuse an old receipt
- **WHEN** current literals are ambiguous or contradict the proposed tracking number
- **THEN** Store SHALL refuse dispatch rather than silently fall back to the old receipt.

### Requirement: Honest coverage and preserved execution fences
The adapter SHALL claim latest-summary coverage only, not full history, real delivery acceptance, prediction or inferred overseas timezone. Existing tenant/session/provider/read-capability/budget/cancellation, completed-slot reuse and unknown-outcome rules SHALL remain. No email, notification subscription, purchase/cancellation or mission capability SHALL be granted.

#### Scenario: Unspecified provider time zone
- **WHEN** the visible latest date lacks an explicit UTC offset
- **THEN** raw date/time text SHALL remain visible and occurred_at SHALL stay absent rather than invent an absolute timestamp.
