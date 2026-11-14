## ADDED Requirements

### Requirement: Exact local Google credential reuse
The experimental trusted-native broker SHALL reuse only an explicitly selected vault API-key refresh credential for the fixed current-account Gmail INBOX-count operation, without per-run approval or secret transfer to server/model.

#### Scenario: Verified account and actual permission
- **WHEN** a compatible current local grant is available for an authorized settings/chat request
- **THEN** native refresh reports the exact supported scopes, verified UserInfo subject/email matches the local expected identity and Gmail profile email matches UserInfo before INBOX counts are returned
- **AND** only four bounded integer total/unread message/thread counts leave the adapter.

#### Scenario: Incompatible credential or provider input
- **WHEN** scope, subject, verified email, mailbox account, refresh-token rotation, HTTP redirect or typed numeric response violates the fixed contract
- **THEN** no success projection or raw provider text/credential is returned
- **AND** credential rejection suspends local reuse until explicit reconnection.

### Requirement: Original request and backward compatibility
Google work SHALL use existing signed current dispatch, local journal, revocation, ACK-loss recovery and original-chat run authority while preserving old GitHub request/receipt/signature bytes and resource records.

#### Scenario: Locked or disconnected and later completion
- **WHEN** a direct personal Gmail INBOX count request has no usable device and later receives its authentic exact signed receipt
- **THEN** the original run waits without occupying a model/worker slot and completes that original tool slot with a numeric summary without another model continuation.

#### Scenario: Cancel, restart or authority loss
- **WHEN** an entered Google read restarts, loses an ACK, or its original run is cancelled/loses authority
- **THEN** the original operation is not replayed, committed late evidence can reconcile privately, and cancelled chat output is not resurrected.

#### Scenario: Old jobs and feature-disabled installations
- **WHEN** old GitHub jobs/profile data is loaded or the broker feature is disabled
- **THEN** GitHub jobs retain their service/scope and signed bytes, older profiles gain no Google tool, and disabled installations expose no Google broker route.

### Requirement: Whole-message affirmative chat authority
The current message SHALL match the bounded complete affirmative count-request grammar before a Google job can be admitted; a model-emitted tool alone SHALL NOT supply authority.

#### Scenario: Negative or meta request
- **WHEN** the current message negates a count read or asks to translate, quote or analyze a count-request phrase and the model still emits the Google tool
- **THEN** Store creates no broker job or new dispatch.

### Requirement: Explicit snapshot credential reconnection
Explicit reconnection of the same snapshot vault/item/Google identity SHALL support newer item and key epochs with the same grant ID and original expiry, after original retained work reconciles.

#### Scenario: Updated credential, root rotation or lost registration acknowledgement
- **WHEN** a fresh authenticated snapshot contains a newer credential revision or root/key epoch and the user reconnects the same selection
- **THEN** the binding advances by an exact previous/new version CAS, pending acknowledgement survives restart and repeats idempotently
- **AND** original committed or entered invocations remain under their original binding and are never replayed.
