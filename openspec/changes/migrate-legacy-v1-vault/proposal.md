# Explicit local old-v1 vault export migration

## Why
F50 needs to carry an actual old `musuri-vault` v1 authenticated export into the current owner-bound v2 vault without running the old server or exposing keys to the assistant.

## What Changes
Add a separately selected local Worker path for old password/MR1 recovery authentication, entire manifest and payload verification, bounded ordinal preview, and new-ID v2 encryption through existing import receipts. Preserve exact old AAD/UTF-8 semantics and reject incompatible input without truncation.

## Impact
Rust core/WASM, vault React Worker/UI and documentation only; no server/schema/dependency/production changes. Raw dumps and source-server completeness/latestness are unsupported. [Contract and evidence](../../../docs/vault-v1-migration.md).
