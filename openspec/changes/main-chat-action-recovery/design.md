## Action ownership
Send and run actions share the existing same-thread mutation fence. The event handler checks the synchronous lock as well as the disabled controls, so form submission cannot bypass it. A pending/uncertain new message cannot race a retry of the preceding question. Retry additionally requires the current latest turn with no active run; persisted can_retry flags on earlier turns do not grant a new action.

Each run action owns a controller and immutable run/attempt/kind identity. A canonical snapshot or SSE turn/state showing a higher attempt settles a retry receipt. For cancellation, a terminal same attempt or a higher superseding attempt settles the old target; this does not claim the cancel caused an independently completed run. The redundant HTTP transport is aborted only after canonical proof. Ownership checks prevent its discarded response/finally from changing or unlocking a newer action. Server expected_attempt remains controlling; no action is replayed automatically.

## Thread deletion
DELETE has one 30-second complete-response deadline. An uncertain receipt triggers one authenticated inventory GET with its existing 30-second bound. Absence confirms local removal; presence or failed inventory retains an unknown outcome and restores usable dialog controls. Thus a silent DELETE plus silent inventory wait is bounded to 60 seconds while the browser is executing timers. Neither path automatically repeats DELETE.

Confirmed absence removes just that thread from current state, selects a surviving thread only if the removed one is still selected, retires its draft/pending slot and aborts any inventory read started before removal. A stale list body cannot resurrect it or discard newer conversations. Late draft/pending callbacks are also fenced against slots absent from the current authenticated inventory.

## Limits
Browser suspension may postpone timers. This is synthetic frontend evidence, not a latency or model-quality claim. Backend/store, external services, real accounts and production deployments are outside this worker. The parent integration performs independent review and browser smoke.
