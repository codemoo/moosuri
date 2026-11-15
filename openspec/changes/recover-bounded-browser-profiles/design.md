# Design

`recover_workspaces` defaults to false and false is omitted from canonical config
serialization, preserving historical deployment pins. Enabled configuration has
a different pin and requires a worker readiness capability. Workspace-only Docker
volumes are named from the operator owner UUID and opaque workspace UUID; labels
include exact project, owner, workspace, account and bounded-profile generation.
The local volume driver must have no host bind/device options. No host path is
accepted from HTTP or model tools.

Schema53 persists an original idle deadline and originating app session digest.
Only current session/account epoch, active account, thread, deployment pin, idle
and absolute deadlines authorize a profile. Restart fences control and location;
claiming recovery does not extend deadlines. A fresh worker opens about:blank,
grants a fresh human capability and leaves the result of prior input unknown.
The first explicit human activity may extend idle time within the original8h.
Model URL navigation remains disabled after human control.

Recovery removes stale owned workers before mounting a profile and removes
Chromium tab/session/service-worker state before startup. It preserves browser
managed persistent cookies and local storage. Session cookies can expire and
crashes can lose recently uncommitted browser state. Normal shutdown asks
Chromium itself to close; the app never reads or edits a cookie database.

Reconciliation uses DB lease authority and exact resource labels, removing
ineligible volumes and old workers. Deleted threads/accounts, revoked sessions,
changed deployment pins, close and expiry do not become eligible on restart.
Deletion failure preserves closed/expired DB state and resource labels for the
next pass; Docker failure is not interpreted as proof of cleanup. Resources under
another operator owner/project are never reconciled. The app must be running
with its configured Docker authority for eventual cleanup; an operator retiring
that deployment must reconcile its resources separately.

The local Docker volume provides lifetime bounds, not a hard disk quota.
Operators need disk capacity/quota/monitoring and reviewed deployment smoke.
Download denial and small browser cache flags remain. No new filesystem quota,
host mount, Docker daemon configuration or network policy is provisioned.
