## Why
F23/F41 custom ingestion could preserve an immutable batch yet get stuck when
`POST /runs` committed before its acknowledgement was lost. Recovery must find
that original run without borrowing another producer's active run or creating
another generation under the same intent.

## What Changes
- Add optional typed start intent and exact historical start receipt on the existing endpoint.
- Bind UUID+expected generation to current owner/connection/config/grant and exact before-position.
- Persist producer start intent before dispatch; recover ACK loss and bounded explicit unstaged inactive retries.
- Add schema48 sidecar plus normal/restore and predecessor fixture integration.

## Impact
Core/Store/server/custom producer only; old `{}` and official poller starts remain.
No model call, new server, provider scope, service/cron or external dependency.
