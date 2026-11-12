## Why

A shared installation needs usable model connections without sharing account data,
echoing credentials, silently changing a selected model, or trusting a catalog as
proof that a particular model/effort can answer.

## What Changes

- Add admin provider profiles with encrypted credentials, immutable revisions,
  bounded catalog discovery and synthetic text-stream verification.
- Add installation default and isolated per-user model/effort selections.
- Add React management and preference screens and file-backed/HTTP/browser checks.
- Keep actual conversation execution, subscription login and user vault separate.

## Capabilities

### New Capabilities
- `provider-settings`: shared connections and independently verified user choices.

### Modified Capabilities
- None.

## Impact

SQLite schema2, separate private provider key, public response DTOs, Rust API and
React routes. No live production credentials, old services or CI changes.
