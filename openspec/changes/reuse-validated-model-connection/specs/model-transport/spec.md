## ADDED Requirements

### Requirement: Bounded per-client connection reuse after fresh validation
The transport SHALL revalidate every current DNS answer before reusing a pooled
connection. Pool reuse SHALL remain within the same configured ModelClient origin
and complete address set with at most one idle connection for30seconds. Changing
transport limits SHALL retire the pool. No shared credentials or automatic retry
shall be introduced.

#### Scenario: Successful sequential tool rounds
- **WHEN** two cleanly completed requests use one unchanged configured client
- **THEN** they may use one existing connection while retaining distinct authenticated requests and receipts

#### Scenario: Cached connection and newly denied destination
- **WHEN** a current resolver answer violates policy even though a prior connection exists
- **THEN** no request is dispatched using that cached connection

#### Scenario: Changed destination set or limits
- **WHEN** the complete validated address set, origin or configured limits changes
- **THEN** the old client pool is retired and the new request uses current validated settings

#### Scenario: Uncertain stream
- **WHEN** a stream is interrupted or has an invalid suffix after a terminal marker
- **THEN** existing cancellation/clean EOF and no automatic replay rules remain enforced
