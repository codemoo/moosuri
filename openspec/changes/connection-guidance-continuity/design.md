# One stable continuation admission

Mission restart, input resume and ordinary mission resume already share a
transactional stable requeue helper. It verifies eligible mission/run states,
rejects unresolved model/external dispatches, retires an attempt32 lease, advances
the bounded carrier attempt and transfers received/applied instructions to that
execution attempt while leaving original receipt `attempt` unchanged.

Provider connection promotion now uses the same helper after restoring its
mission to queued inside the existing writer transaction. Connection selection,
owner capacity, current binding checks, exact wait identity and lifecycle event
publication retain their existing owners. A rejected requeue rolls back its
local transition with the caller transaction. No schema or external effect is
added. Native model-mail paths remain outside this change; their profiles do not
currently admit live instructions.
