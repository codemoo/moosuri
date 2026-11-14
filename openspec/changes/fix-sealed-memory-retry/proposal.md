## Why

An explicit retry of a sealed local memory command fails before its durable intent
can resume. An unrelated awaiting classifier-route guard runs first, even though
a validated memory intent already exists. The existing outcome regression fails
on the published pre-personal-provider baseline.

## What Changes

- Resume the typed memory intent under current authority before model client
  construction and unrelated classifier routes.
- Reuse the existing delivery lock, intent identity and Store commit checks.
- Preserve existing provider checkpoint, dispatch and unknown-outcome rules.

## Impact

No schema, provider, dependency, approval or automatic retry changes. This does not
enable replay of unknown remote work or broaden memory extraction.
