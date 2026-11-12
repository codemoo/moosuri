## Why

Contributors should share reusable capabilities through GitHub while end users discover them in the existing agent suggestions/capability interface without managing a separate marketplace.

## What Changes

- Add a small standard skill package, static Git-based catalog and native Rust installer.
- Bind installation/version/credentials to each user while caching immutable public bytes.
- Integrate truthful readiness, install/use, dismiss and updates in existing cards.
- Export private learning by reconstruction with synthetic examples, never raw traces.

## Capabilities

### New Capabilities
- `community-skill-discovery`: Git contribution, card discovery and per-user package lifecycle.

### Modified Capabilities

None.

## Impact

R25/R26/R27, F25/F26/F32/F65/F66/F67/F68. See [community design](../../../docs/community-skills.md) and [authoring](../../../community/README.md). Product implementation and actual publishing remain pending.
