# Design

New ordinary chat receives a separate version1 CalendarRunProfile in schema30; the immutable
existing RunToolProfile and pre30 mission/chat grants are not upgraded. The profile captures
current connection epochs, managed app revision/client identity, server UTC reference and submitted
IANA timezone. Credentials remain inside the trusted service. General model tools can only read.

The existing isolated mission current-text request offers mission_chat_decision OR
calendar_chat_decision, accepts exactly one function call, and atomically acknowledges that typed
domain outcome with the model step. A sealed Calendar proposal stores one trusted action UUID.
Its normalized civil fields and exact current-text quotes are proposals, not arbitrary mutation
payloads. Store verifies explicit numeric/date/clock consistency against the frozen reference;
missing or ambiguous civil time, duration, account or target produces a terminal clarification.
The semantic model interpretation is not proven by substring checks.

Updates/deletes require exact current title and date. A real provider page of at most five must
be complete and have exactly one matching eligible event; partial or duplicate coverage clarifies.
This initial update slice changes title or explicit times on that same civil date; moving across
dates needs a distinct target/new-date proposal and currently clarifies. A fresh event supplies unchanged full fields and must match that observed ETag. No field is silently
truncated into a write. Creation requires explicit start and end/duration; changing start also
requires explicit end/duration. Multiple writes, source-derived values and cross-turn pronouns are
outside this slice. Korean/English parsing is intentionally bounded by explicit clock/date checks.

Immediately before dispatch, one writer transaction checks the run/step, frozen original session
and epoch, sealed plan, connection/app pins, target proof and ETag, then inserts the existing26
Calendar action and its run link and consumes the plan. Only the returned dispatch invokes the
provider once. A retry of committed work renders the same minimal ledger receipt; unknown work
uses exact read-only lookup. The existing nonce-bound service settlement records late minimum
receipts even when chat delivery is revoked. No model final round is needed for the mutation receipt.

Successful read admission records Calendar withdrawal dependencies. Opaque unpredictable event
handles are owner/run/step/call/range bound; continuation tokens require actual prior-page lineage.
Inherited conversation output inherits these dependencies. Disconnect/reconnect, reconnect-required,
managed-app replacement and restore withdraw relevant Calendar context/continuations/plans;
minimum historical action receipts survive. A minimum action-origin tombstone outlives deleted
conversations so late settlement cannot restore private event/payload fields after the run link is gone. Normal token refresh does not withdraw history.
Logout/auth revocation preserves stored completed conversation history and fences unpublished IO
and unapplied writes by the original session/epoch. Restored profiles, observations and plans are
removed and all imported credentials/unfinished actions remain quarantined.

Artifact persistence rejects Calendar context until independent provenance withdrawal is supported.
Literal memory commands still use only current-user quoted facts and their existing stored guard;
retrieved Calendar fields and history are never supplied as new literal command authority.

The direct HTTP forms continue to use the same CalendarService and direct-owner26 ledger contract.
Google origin and no-notification flags remain fixed. Mail has no send tool; final human preview
approval remains mandatory for every email send.


Cancellation records a monotonic `withdrawn` bit in the existing schema30 action-origin tombstone
and clears private payload/event, sealed proposal, observations, checkpoint and current-run output
inside the same writer transaction as the canceled state. A database trigger prevents resetting that
bit. Both late settlement and direct action projection require an unwithdrawn origin, regardless of
subsequent Retry/claim transitions. The current-request intent row remains `passed` with its private
proposal removed, so removing the Calendar plan cannot reopen classification. Already committed
UUID/nonce/state/minimum receipts remain; pre-dispatch cancellation creates no provider action.
Safe ordinary reads/chat can continue on Retry without restoring the retired Calendar command.
