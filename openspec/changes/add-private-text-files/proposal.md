# Add bounded private UTF-8 file attachments

## Why

Users need to attach a small document and ask questions about its actual immutable contents, while preserving account ownership, retry association, deletion provenance and the distinction between source data and instructions.

## What Changes

- Add owner media metadata and bounded UTF-8 original/extraction for TXT, Markdown, CSV and validated JSON.
- Reuse immutable IDs, nonce/auth fences, shared quotas, durable progress, unknown ACK recovery, tombstones and transitive chat withdrawal.
- Connect independent file upload, safe text preview, original download and library selection to real chat/retry.
- Pass labelled exact revision/SHA256/byte evidence to the model while withholding all source-derived tool/write authority.
- Preserve completed files and deleted provenance through encrypted snapshot restore; quarantine pending upload leases.

## Impact

F10/F14; reserved assistant schema0027 after Calendar26, core DTO, Store/agent/server/React. No dependencies or external provider calls. File24KiB, combined4 selected, shared64MiB/account,4096 IDs. PDF/audio/Office/binary parsers, collection ingestion/automatic memory and whole-media completion remain pending.
