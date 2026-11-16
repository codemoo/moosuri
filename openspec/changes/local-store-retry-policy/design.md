# One loop, explicit error policies

`store_retry` owns five attempts with 20 ms between retryable failures. Local
operations retry Busy only; Unavailable may mean a lost acknowledgment and returns
unchanged. The existing run-authority policy also retries Unavailable. This check
can durably settle revoked/expired runs in Store; it never dispatches work.
Revocation, conflict, budget and all other errors return immediately.

The runner keeps its operation-specific payload, usage/output ordering and signal
timing. Mail send and reconciliation IO finish before the local retry closure is
created; only the same receipt/observation is persisted again. Both mail paths now
omit their former unused sleep after the fifth failed attempt. Their admission,
final-preview approval, outcome uncertainty and send count remain unchanged.

The source collector's separate three-attempt envelope budget is outside this
change. Domain-specific behavior is not flattened into a configurable retry
framework. Existing helper names inside the private runner preserve its
planner, memory, mission and continuation call sites.
