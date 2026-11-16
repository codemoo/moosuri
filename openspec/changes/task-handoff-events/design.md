# Transactional target publication

One Store helper resolves only the affected carrier's authenticated-admission
`create_now` origins, with matching tenant and account. It compares the current
instruction target with each origin's latest durable state/turn event and emits
one state update only when different. Missing retained projections receive one
replacement from the lifecycle mutation; correctness does not depend on retaining
old events or scanning accounts from SSE reads.

Existing chat terminal transitions and direct mission completion, control,
expiry, input/connection/native wait and resume, model-capacity dispatch and
source withdrawal call this helper in their existing writer transactions.
Receipt changes keep their existing publication. Nested calls are deduplicated
against the event committed in the same transaction. Deleted/quarantined tasks
remove their target instead of restoring authority or private task data.

The existing server SSE reader observes committed chat events after its cursor,
waking on Runtime changes or its 250ms fallback. This is conversation event
delivery, not a new Push-notification mechanism. Reads themselves remain free of
mutation. The original admission run stays completed while its target follows
the real carrier; a false target lets the composer use ordinary next-message
admission without a forced page refresh.
