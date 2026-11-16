# Shared planner dispatch and mission transport

## Why

Artifact, memory and mission/Calendar classification duplicate preflight,
subscription readiness, dispatch admission, streaming and local checkpoint logic.
A durable personal mission also reconstructs its transport between each round,
throwing away the already established HTTP connection.

## What Changes

- Share the bounded isolated-planner transport, keeping each domain parser and
  its exact Store admission/authority intact.
- Reuse the current personal mission transport after checking its current lease
  and rotating-token readiness for every round.
- Check actual model connection identity with the existing completed mission
  workflow and retain failure/cancellation/protocol regression coverage.

## Impact

No schema or public API migration, new model call, capability or approval flow.
Only the existing read/write domain grants decide execution.
