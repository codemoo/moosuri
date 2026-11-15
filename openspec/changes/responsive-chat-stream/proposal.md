## Why
A half-open browser SSE transport can wait forever, six disconnected connections exhaust recovery even after successful delivery, and every fresh POST forces an extra history fetch and drops loaded older pages.

## What Changes
Bound SSE connect/header completion to 15 seconds and raw-byte inactivity to 45 seconds. Bound read-only JSON GET requests to a single 30-second total deadline across headers and body so canonical snapshot recovery also terminates. Mutation timing and dispatch remain unchanged. Count consecutive unhealthy reconnects, accepting new validated cursor events or a decoded comment heartbeat after 15 seconds as recovery. For fresh sends, retain the live stream/history when it confirms the same request within one second; otherwise reconcile the canonical page. Explicit same-ID resubmission continues to reanchor its snapshot.

## Impact
React chat transport and synthetic tests only; no Rust/store/API schema, model calls, source access or credential permissions change. Existing authority, reset and account lifetime fences remain controlling.
