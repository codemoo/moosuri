# Browser chat stream responsiveness

Synthetic-only React work on the 2157adc integration base. No model/provider calls, browser accounts, production data or Rust/store changes.
The later integrated browser and Linux checks are recorded separately in the
[response pipeline review](2026-11-15-response-pipeline.md).

## Defects and changes
- An opened stream previously waited forever in reader.read when a connection stopped delivering bytes. SSE now has a 15s connect deadline and a 45s raw-byte read deadline; the server's 15s comment keepalive counts as activity. Scope closure releases a transport even when mock fetch ignores AbortSignal and discards a late response body.
- A lifetime counter previously stopped after six disconnected connections despite successful events between failures. Six consecutive unhealthy failures now stop recovery. New validated event cursor progress or a decoded comment after at least 15s connection lifetime resets the budget; immediate open/comment/EOF does not.
- Every successful ordinary POST previously cleared history and replaced the live stream with a snapshot round trip. A fresh send now awaits its own streamed receipt for at most one second, retaining earlier pages and the same stream. Explicit same-ID resubmission and missing receipts retain canonical snapshot reconciliation; POST output is never merged into deltas.

## Verification
Six relevant React suites passed (65 tests), followed by a focused passing recovery check after adding immediate-comment failure and manual focus recovery assertions. Production web build (including TypeScript) and strict named OpenSpec validation passed. `git diff --check` passed. Controlled tests observe half-open and quiet-heartbeat transports, late fetch disposal, account close, more than six genuine recoveries, bounded open-only failures, POST/SSE acknowledgement races with older history and one POST under double click, missing receipt fallback, and logout during its receipt wait.

The preexisting Chat.test scroll fixture mistook synthetic dimension replacement for reader intent; it now delivers resize before scroll, preserving the same no-jump assertion. The media-library query now uses its existing accessible label. Both old failures were reproduced with the original api.ts/ChatPanel.tsx before fixing those stale fixtures. Schedule continuity fixtures emit no events; their receipt-count waits allow two seconds for the deliberate one-second fallback, retaining all revision and selection assertions.

## Snapshot recovery follow-up
Independent review found that a stream deadline could still lead to an indefinitely stalled canonical GET. Read-only JSON GETs now use one 30s deadline for fetch headers and complete body, with an owned reader that can cancel a stalled mock body. Expiry is an ordinary 504 request_timed_out failure; account closure is still AbortError. Late responses are cancelled and cannot update the current conversation. POST/other mutation timing and dispatch are unchanged.

Focused observations cover stalled GET headers, headers arriving at 20s followed by a body stall at the original 30s deadline, prompt account closure during connecting/body read, one unchanged POST still pending after 45s, and chat snapshot timeout leaving loading followed by explicit GET-only recovery and a fenced late old snapshot. Six relevant React suites passed (71 tests), TypeScript checks and strict named OpenSpec validation passed, and `git diff --check` passed. Draft-recovery fixtures now clone held GET responses per request: overlapping StrictMode requests previously shared one body, so correctly cancelling the abandoned response also cancelled the current mock response. This corrects fixture transport ownership without changing privacy assertions.

## Limits
Browser suspension can defer timers; existing focus/online/visibility recovery still reloads an authenticated snapshot. No real-network or provider latency claim is made. The runtime's 32ms output checkpoint batching and server event authorization/backpressure remain unchanged. CI stays disabled.
