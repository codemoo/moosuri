# Consolidate local Store retry policy

## Why

Seven copies of bounded retry mechanics in the runner and mail receipt paths can
drift during maintenance. Feature stability includes consolidating shared decisions
and removing their replaced implementations, not only adding regression checks.

## What Changes

Use one bounded local Store retry implementation with two explicit policies:
Busy-only for local operations, and Busy/Unavailable for the existing run-lease
authority recheck. Preserve original caller payloads, fences and external IO order.

## Impact

F03–08, F19–21, F24, F27, F33–37. Agent internals only; no new API, migration,
external action, model call or permission dialog. CI stays disabled.
