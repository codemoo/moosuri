# Finish acknowledged mission work before remote token readiness

F27/P3 acknowledged mission recovery currently constructs a personal model client
before consuming the saved completed proposal. An expired token and temporary
OAuth discovery outage can fail a mission whose remaining work is entirely local.
Move model construction/readiness after guarded local recovery and keep it before
every new inference. Original owner, binding, source, deadline and effect receipts
remain mandatory; no credentials, budgets or prior dispatches are reset.
