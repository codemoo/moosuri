## Scope and authority

Fixed content only, not an agent mission. An owner action seals a persisted
tenant/account/auth_epoch/revision grant; logout and background scheduling are
separate. Enqueue/claim/final publication recheck account and current schedule.
Pause/edit/revoke first fences the old nonce/revision; committed inbox receipts
remain. Native trusted clock entry points serve fake-clock tests and are never
HTTP/model tools. No provider binding or synthetic UserContext exists.

## Time and queue

Chrono plus a compiled chrono-tz IANA database implement daily gap skips/first fold
offset, with original rule/timezone/version retained. UTC interval anchors never
drift. A bounded recurrence planner computes the latest valid missed slot and
summarizes older skipped ranges. One claimed+one latest queued occurrence is
enforced by partial unique indexes; stable UTC/local-slot uniqueness spans rule
revisions. Per-owner persisted claim order prevents starvation. Resume/edit start
in the future; expired/paused past one-shots cannot appear delivered.

## Publication and recovery

Claim has no external effect. The local notification insert and occurrence receipt
update commit together; a repeated delivered nonce returns the same receipt.
Ordinary startup requeues/coalesces uncommitted30second claims and leaves delivered
rows exact. Root restore/cutover instead calls the quarantine hook in its existing
activation transaction, retaining receipts but disabling old grants until owner
action. This worker adds no restore/latest-checkpoint claim.

## UI and limits

Creation/preview/edit/control and inbox/settings use current-account RequestScope,
synchronous mutation ownership and stale-response generations. Edits retain their
CAS revision through refresh. Quiet hours mark inbox state, not a fictional Push
send. Bounded queries and queue counts do not imply history GC. Root's active-
account predicate and schema18 integration are explicit dependencies; production
tool authority and old backup/knowledge/community behavior remain separate.
