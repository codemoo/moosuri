## ADDED Requirements

### Requirement: Owned main-chat mutation admission
The browser MUST prevent overlapping same-thread new-message and cancel/retry admissions, preserving the next draft and immutable uncertain send identity. Retry MUST remain limited to the latest eligible turn with no active run, even when an earlier saved run retains an old can_retry flag.

#### Scenario: Retry and next question race
- **WHEN** retry admission or a new-message receipt remains unresolved
- **THEN** the opposite operation cannot dispatch, including through direct form submission, and the next draft remains available

#### Scenario: Later turn arrives through SSE
- **WHEN** a later turn is admitted while an earlier failed turn retains can_retry
- **THEN** the earlier retry control is retired and cannot dispatch a retry

### Requirement: Canonical run-action receipt settlement
A canonical snapshot or SSE run transition MUST release a redundant run-action HTTP wait when it proves a higher retry attempt or a terminal/superseded cancellation target. Discarded responses and cleanup MUST remain fenced to their immutable action owner and MUST NOT overwrite or unlock a subsequent action. No write MUST be automatically replayed.

#### Scenario: Retry receipt hangs after execution starts
- **WHEN** the HTTP retry receipt stalls but SSE confirms the same run's higher running attempt
- **THEN** the redundant HTTP wait is cancelled and the user can immediately cancel that confirmed attempt using its actual expected_attempt

#### Scenario: Cancel receipt hangs after its target settles
- **WHEN** canonical state confirms the cancel target terminal or superseded
- **THEN** the receipt wait releases and the next explicit message is available once no active run remains, while a late HTTP body cannot reopen the old target

### Requirement: Bounded consistent thread deletion
A foreground thread DELETE MUST have one 30-second complete-response deadline. Unknown outcomes MUST reconcile through bounded authenticated GET without automatic DELETE replay. Confirmed removal MUST preserve newer inventory/drafts, cancel pre-removal list responses and retire deleted slots. A failed or inconclusive reconciliation MUST restore usable dialog controls without claiming definite deletion failure or success.

#### Scenario: Lost deletion receipt
- **WHEN** DELETE stalls after the thread may already have been removed
- **THEN** the client checks inventory once, confirms removal only on absence and never automatically sends another DELETE

#### Scenario: Deletion and inventory both stall
- **WHEN** DELETE and its subsequent inventory check both fail to finish
- **THEN** their respective 30-second deadlines release the blocking controls and keep the result unknown

#### Scenario: Inventory changes during deletion
- **WHEN** newer conversations arrive before a held deletion receipt completes and an earlier inventory body arrives afterward
- **THEN** only the deleted thread is removed, newer conversations and the current selection survive, and the stale inventory body is discarded

#### Scenario: Authoritative absence precedes the deletion receipt
- **WHEN** an authenticated thread boundary confirms the target absent while its DELETE or inventory receipt remains pending
- **THEN** the matching owned transport and deletion lock release immediately, and its late response/cleanup cannot unlock or close a newer explicit deletion
