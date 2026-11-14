## ADDED Requirements

### Requirement: Private learned operations require independent adopted receipts

The system SHALL learn only supported exact-calculation or saved-source lookup routes from two distinct finalized same-owner canonical inputs and linked successful real receipts. The final saved output SHALL include the exact adopted operation postcondition. Instruction reads, replay, other owners, unfinished execution and prose completion SHALL NOT count as independent proof. Learned operation proof SHALL NOT imply semantic mission success.

#### Scenario: Two successes and fresh C

- **WHEN** A has one finalized adopted supported chain, a replay of A occurs, and B then has a distinct canonical finalized adopted chain
- **THEN** the route remains candidate through replay and becomes usable after B
- **AND** execution on C binds C operands or a fresh search result, without A/B source IDs

### Requirement: Reuse preserves current authority and durable execution

The system SHALL expose only usable current-owner pinned routes through current skill discovery. Each actual underlying operation SHALL consume the existing cumulative budget and preserve dependencies and linked durable receipts. Replay SHALL return the existing receipt without additional execution. Existing runs SHALL NOT gain new route pins.

#### Scenario: Replayed lookup

- **WHEN** a pinned usable route searches C and reads its current first hit, then the same parent slot is replayed
- **THEN** two linked underlying receipts and two tool charges exist exactly once

### Requirement: Stale or removed routes cannot publish delayed output

The system SHALL stop reuse on schema drift, invalid proof, source edits/deletion, user disable/remove or three attributable route failures. Source invalidation/removal SHALL erase retained private proof claims and invalidate derivative output/slots. Infrastructure failures, cancellation and missing evidence SHALL NOT count as attributable failure.

#### Scenario: Training source is removed

- **WHEN** A's source is deleted after C reused the learned lookup
- **THEN** private proof hashes are removed and derivative/pinned route output is invalidated
- **AND** stale route execution or late publication is blocked

#### Scenario: A learned answer is actually delivered to a text-only follow-up

- **WHEN** fresh C uses a learned exact operation and its answer is delivered into D's selected model context
- **AND** D has no admitted tools and completes, while a further text-only descendant is interrupted, retried and holds a late output
- **THEN** route removal, disable or training-thread withdrawal purges completed derived answers and blocks the held output and retry
- **AND** valid text-only continuation works before withdrawal, with no new tool pin or authority
- **AND** discovery-only unrelated conversations and other owners remain unchanged
