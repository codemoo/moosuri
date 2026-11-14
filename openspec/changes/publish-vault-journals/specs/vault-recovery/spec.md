## ADDED Requirements

### Requirement: Opt-in non-sealing off-host journal capture
The vault service SHALL periodically capture actual DB/journal under the exact HTTP Database mutex and SHALL release that writer before semantic replay, encryption and transport. Source sessions and mutation authority SHALL remain live; no source seal or recovery identity SHALL be created/read.

#### Scenario: Frozen capture while source continues
- **WHEN** publication takes a consistent source capture
- **THEN** later source appends remain usable through the independent journal descriptor
- **AND** encryption/SSH consumes only frozen files after releasing the writer.

### Requirement: Independent monotonic encrypted-only receiver
An explicitly initialized receiver SHALL pin source, baseline and public recipient, validate every typed journal hash-chain step and exact prior byte-prefix digest, and retain encrypted full journal plus immutable linked receipts only. Fork, rollback, foreign source, invalid frame, truncated content, extra tail and current artifact corruption SHALL fail closed.

#### Scenario: ACK loss after durable receipt
- **WHEN** a receipt committed before source ACK loss and identical captured content is retried
- **THEN** receiver returns the same checkpoint/full receipt and does not advance generation.

#### Scenario: Interrupted generation before receipt
- **WHEN** the process stops before ledger publication
- **THEN** encrypted residue is not accepted/current, and a later valid retry cannot overwrite an accepted generation or receipt.

### Requirement: Independent status and bounded SSH lifecycle
The source SHALL use only fixed pinned-SSH argv with restricted alias/target tokens, bounded IO/deadline/cancellation and transport process reap. A separately accessed target-status SHALL return the exact historical head pin and retained input paths. No web endpoint or arbitrary command/path SHALL be accepted.

#### Scenario: Source lost
- **WHEN** the operator independently queries the target and retains the returned exact pin
- **THEN** existing restore-historical can consume the retained baseline and full journal with offline identities, retaining its exact-prefix/semantic/quarantine contract.

#### Scenario: Target unavailable
- **WHEN** periodic publication fails
- **THEN** source writes continue and status reports not_protected/lagging with an explicit recovery gap
- **AND** no zero-loss or independent-world latestness is claimed.
