## Decisions
The server emits SSE comments every 15 seconds. A 45-second raw-read deadline tolerates two missed heartbeats; comments count as activity even without model output. Connecting has a separate 15-second deadline. Cancellation also settles a fetch that ignores AbortSignal, and late responses are cancelled.

Canonical snapshot requests use the shared read-only GET path, with one 30-second deadline spanning connecting and reading JSON. An owned body reader allows deadline/account cancellation even when the test transport ignores AbortSignal. Deadline expiry is a normal 504 request_timed_out failure, so existing loading/error/explicit-refresh handling remains available; account closure remains AbortError. Late headers are cancelled and cannot publish a result. Mutation requests retain their prior timing, dispatch and uncertain-outcome behavior.

A maximum of six consecutive failed connections remains. A new validated monotonic event resets failures immediately. An idle connection resets failures only after a decoded comment arrives at least 15 seconds after opening; HTTP headers or immediate comments followed by EOF do not prove recovery. Focus/online/visible recovery retains its existing fresh authenticated snapshot.

Fresh POST responses have no event cursor and never merge into streamed output. A same-request receipt already applied through SSE, or arriving within one second on the current stream, clears the pending request without a GET/reconnect or history loss. The existing snapshot path handles missing receipts and explicit duplicate resubmissions. Waiters are released on stream/effect closure; thread/account aborts fence delayed fallback work.

## Limits
Background timer suspension can delay the deadlines; visibility/focus still initiates recovery. These synthetic checks establish recovery and removal of an extra normal-send round trip, not real-provider time-to-first-token performance. Runtime 32ms checkpoint batching is unchanged.
