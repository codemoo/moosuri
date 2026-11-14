## Why
F41 needs a useful opt-in local work-session producer without reviving old services
or assuming private client formats. Explicit human request evidence should reach
an owner's existing custom source connection with a durable receipt/cursor.

## What Changes
- Add one Rust `work-sessions codex` command with mandatory source/destination/token/state inputs.
- Parse a public pinned Codex JSONL subset with strict human session/turn provenance.
- Reuse the custom ingest protocol with immutable pending batch and cursor recovery.
- Add focused local-file→mock HTTP outcomes and honest supported/unsupported documentation.

## Impact
CLI and connectors only; no server, assistant schema, automatic service, model
call, source autodiscovery or new dependency. Claude/legacy/unknown formats remain unsupported.
