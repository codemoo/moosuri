# Preserve unsent instruction drafts across task continuation

## Why

A task continuation changes its carrier attempt. An instruction draft written
before that change remains pinned to the old attempt, leaving its send button
disabled even though the same task is accepting instructions again.

## What Changes

- Follow a newer attempt only for an unsent draft on the same run and thread.
- Persist that forward target so stale views cannot rewind the draft.
- Preserve an uncertain submitted request's original UUID, attempt and text.
- Keep different carriers isolated and require an explicit send for new drafts.

## Impact

The shared React instruction composer and its integration tests change. No
server endpoint, execution grant, model call, approval or schema is added.
