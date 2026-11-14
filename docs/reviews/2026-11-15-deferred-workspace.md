# Deferred workspace screens

P0/F54 and P7/F09: the default chat and shared account/draft lifetime stay eager.
Secondary settings, knowledge, calendar, mail, missions, schedules and suggestions
use React lazy boundaries. Only public code/CSS is deferred; private API calls and
account scoping are unchanged. The page heading/navigation stay outside the
loading/error boundary. A failed screen offers return to chat or explicit app
reload, with no automatic reload, submission or retry of an external action.

The existing public PWA shell still precaches the build's public assets, including
deferred chunks, in the background. This change reduces the foreground module
graph's parse/execution and critical resources; it does not promise lower total
PWA installation bytes, lower model latency or a measured device speedup.

The App suite passed25 cases; its remaining keyboard test assumed the Calendar
body existed as soon as the eager page heading changed. Waiting for the actual
body control repaired that test, which then passed alone. Typecheck and production
build passed. No successful unrelated cases were repeated for that wait correction.

An isolated Chrome session loaded the actual production build with synthetic API
responses. Service-worker installation was intentionally blocked to isolate the
foreground module graph: the initial page requested only the entry JS/CSS, without
secondary screen modules. Blocking the Calendar JS download produced the visible
error state. Returning to chat retained the original unsent draft. This is a
network-fault/browser observation, not a device benchmark or live account test.
The unchanged generated service-worker asset list includes the emitted secondary
chunks; actual full PWA/update acceptance remains under its existing contract.

Final integration measured the entry module with the same Python gzip settings:
610,903 → 383,331 bytes (37.25% smaller), gzip172,238 →
115,639 bytes (32.86% smaller). These are module sizes,
not elapsed-time benchmarks or total PWA bytes. All25 emitted JS/CSS paths are in the
public worker allowlist. Final integrated typecheck/production build passed. No
dependency, backend, schema, model/effort or CI change is included.

The final pinned offline Linux capsule built the assistant with these actual
production assets embedded and rechecked every source hash. No previously passed
Rust behavior suite was repeated for this frontend-only change.
