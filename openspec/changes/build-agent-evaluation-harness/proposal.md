## Why

The rewrite needs evidence of memory quality, coherent conversations, successful missions and useful skills. Copying legacy tests would preserve obsolete internals and duplicate checks.

## What Changes

- Build a Rust evaluator around synthetic worlds and actual runner/store/provider boundaries.
- Separate deterministic contracts, model quality, latency, device and live-provider checks.
- Use outcome/receipt graders, bounded budgets, frozen comparisons and value-based test selection.

## Capabilities

### New Capabilities
- `agent-evaluation`: Reproducible and honest assistant capability evaluation.

### Modified Capabilities

None.

## Impact

R23/R24, F54 and all feature-to-scenario links. See [testing](../../../docs/testing.md), [examples](../../../docs/test-examples.md) and [scenario catalog](../../../audit/evaluation-scenarios.tsv). Only the planning/checker artifacts are implemented in this preparation; the evaluation runtime remains pending.
