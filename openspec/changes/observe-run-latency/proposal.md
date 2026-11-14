## Why

P0/F54 needs real measurements that distinguish queue/context/model/tool work and
useful final-body/terminal observation without exporting private traces or turning
a tiny synthetic suite into a performance benchmark.

## What Changes

- Record numeric per-attempt observations through existing Store and Runtime paths.
- Preserve observed usage, original model/effort/profile, retry counters and budgets.
- Add owner diagnostics and bounded owner/admin aggregates to existing usage surfaces.
- Expose missing samples, original outcomes and phase/restart limits explicitly.

## Impact

Core numeric DTOs, private Store JSON extension, bounded Runtime hooks and Usage UI.
No schema, external telemetry, dependency, framework, model call or authority change.
