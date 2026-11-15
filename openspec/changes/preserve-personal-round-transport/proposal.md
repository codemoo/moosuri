# Preserve personal foreground transport across rounds

## Why

Personal foreground continuations and isolated planners reconstruct their model
client after each durable authorization check. This discards an already validated
HTTP connection and repeats connection establishment. Separately, failed or
cancelled OAuth discovery leaves an acquired refresh admission for 45 seconds,
while a retry gives up after 35 seconds despite no token exchange being sent.

## What Changes

- Retain one explicit run-local HTTP transport lineage through personal planner
  and answer/tool rounds while replacing the bearer from durable current state.
- Release only a matching, undispatched refresh admission when discovery fails or
  its caller is dropped; preserve rotating-exchange uncertainty after dispatch.
- Add real loopback socket, bearer-rotation, revocation and prompt-retry evidence.

## Impact

F01/F02/F03 personal model and owner-isolation contracts. No schema migration,
provider inference retry, global pool, shared credential state, model/effort
change, live-account call, deployment or CI enablement.
