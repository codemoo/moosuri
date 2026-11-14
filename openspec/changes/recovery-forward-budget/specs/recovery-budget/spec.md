## ADDED Requirements

### Requirement: Explicit new forward request allowance
A historical accounting gap SHALL remain unknown in immutable restore provenance and historical usage views. A trusted stopped local CLI MAY explicitly allocate new inference requests and validated STT input duration for the current restore identity without changing old usage, reservations, credentials or imported work quarantine. Allocation SHALL use the original Store writer admission and a bounded idempotent request UUID.

#### Scenario: Restored application resumes useful new work
- **WHEN** the operator supplies the exact current restore ID and a new request UUID with bounded allowances
- **THEN** fresh authorized shared/personal chat, capability probes, automatic memory and STT MAY dispatch within that allowance
- **AND** historical accounting SHALL stay unknown and old usage/reservation lower bounds SHALL remain intact.

#### Scenario: Allocation retry and later restore
- **WHEN** the exact allocation request is retried
- **THEN** no extra allowance SHALL be granted and changed caps with that UUID SHALL reject.
- **WHEN** a later historical or sealed restore establishes a new installation/restore identity
- **THEN** copied allowances SHALL not authorize it and any ancestral historical unknown SHALL remain effective.

### Requirement: Transactional permanent dispatch commitment
Each actual inference transport round SHALL commit a unique immutable reservation in SQLite before transport, atomically with existing durable dispatch state when present. STT SHALL also reserve validated input duration. Unknown outcomes, cancellation, timeout, restart and failed acknowledgement SHALL never automatically refund a commitment. Duplicate commitment SHALL not authorize replay. Existing per-run and transport bounds SHALL remain.

#### Scenario: Last slot and concurrent requests
- **WHEN** two dispatches contend for one remaining request
- **THEN** exactly one MAY commit and the other SHALL report allowance exhaustion without provider IO.
- **AND** the committed final request's heartbeat SHALL not require another remaining slot.

#### Scenario: Capability probes and catalogs
- **WHEN** a tool capability probe performs two transport rounds
- **THEN** both rounds SHALL consume allowance with exact current binding/authority validation.
- **WHEN** a catalog is listed or credentials are registered/refreshed without inference
- **THEN** no inference allowance SHALL be consumed.

### Requirement: Honest forward allowance status
CLI/API/UI SHALL distinguish historical accounting unknown, forward allocation required and forward allowance exhausted. Displayed forward counters SHALL describe committed inference requests and validated STT duration, not reconstructed historical billing, money or token ceilings.

#### Scenario: No misleading financial guarantee
- **WHEN** the operator views or allocates an allowance
- **THEN** the product SHALL state that request-count/STT-duration limits do not guarantee provider cost or token ceilings and remote processing may outlive local cancellation.

#### Scenario: Charged account deletion and latest-journal replay
- **WHEN** an account with forward dispatch commitments is deleted or its deletion is replayed onto a historical snapshot
- **THEN** private tenant data SHALL purge and startup/restore SHALL succeed
- **AND** immutable content-free accounting commitments SHALL remain as opaque ID/count/duration lower bounds, without refund or renewed dispatch authority.
