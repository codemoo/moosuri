# Design

`runner::planning::PlanningSession` owns shared preflight, per-round personal
readiness, bounded dispatch and retry of the same local checkpoint. `Admission`
is a closed enum distinguishing normal local intent from mission-chat admission;
only the latter calls `begin_mission_chat_step`. Neither inference nor an ambiguous
dispatch acknowledgement is retried. Parsing and construction of memory targets,
artifact grants and Calendar/mission decisions stay in their current modules.

The original current-user request remains isolated from retrieved evidence and
previous answers. The shared layer does not decide the selected domain or tool.
Its only artifact behavior change is using the existing bounded local commit retry
also used by the other planners. Model transport still carries no write authority.

Durable mission rounds use `client_for_round` just like foreground continuations.
It resolves `subscription_access` and token-refresh CAS before renewing the current
client. This preserves the connection pool without retaining stale authorization
or changing the original mission budget. Connection identity is measured from
actual requests to a synthetic official-protocol provider, not an internal flag.
