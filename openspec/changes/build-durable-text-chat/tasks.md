## 1. Durable store
- [x] 1.1 Add idempotent turns, attempts, opaque leases and restart classification.
- [x] 1.2 Add incremental events, deletion/cancellation fences and usage receipts.

## 2. Runtime and API
- [x] 2.1 Implement bounded supervised execution, context projection and safe outcomes.
- [x] 2.2 Add authenticated chat/SSE/actions/usage APIs and graceful shutdown.

## 3. React
- [x] 3.1 Implement composer, smooth streaming, paged history and reconnection.
- [x] 3.2 Implement retry/cancel, account fencing and clear usage states.

## 4. Acceptance
- [x] 4.1 Verify meaningful storage/HTTP/React/failure scenarios and actual browser flow.
- [x] 4.2 Independently review authorization, leases, deletion, accounting and retries.
- [x] 4.3 Verify Linux, public source/secret/spec checks, then commit/push with CI off.
