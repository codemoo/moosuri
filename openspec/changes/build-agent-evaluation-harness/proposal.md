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

R23/R24, F54 and all feature-to-scenario links. See [testing](../../../docs/testing.md), [examples](../../../docs/test-examples.md) and [scenario catalog](../../../audit/evaluation-scenarios.tsv). The initial `moosuri-eval` runtime executes a bounded public regression subset through authenticated HTTP, Runtime and fresh SQLite, with independent state/receipt grading and negative controls. It does not establish the legacy baseline, semantic quality,32-world pilot, non-inferiority or full scenario acceptance. See [runtime scope](../../../docs/agent-evaluation.md).
