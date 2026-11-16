# Persist the received round before interpreting or executing it

`runner::commit_round` owns the local Store retry envelope. The shared retry policy
allows at most five identical writes, waiting 20 ms only between Busy results.
Each write keeps Store's current lease/authority checks and transactional response/
tool-slot persistence. Unavailable, conflict, cancellation, invalid input and
budget errors return to the original caller without retry. This does not create
a new dispatch or widen any tool authority.

Foreground continuation, mission verification, planner domain commits and Claude
terminal handling remain distinct. Notifications and provider call order stay
with those callers. The helper returns the persisted context, not a success or
completion judgment about the user's task.

The synthetic regression holds the real Store actor and fills its bounded queue
without a new production hook or sleep-based race. One already admitted mission
round must survive temporary pressure, then reopen and finish from its saved tool
slot. A second case places owner cancellation ahead of the retry: no checkpoint
or tool result may revive the cancelled task. Existing stream, guidance, planner,
mission terminal/recovery and Claude contracts cover the surrounding callers.
