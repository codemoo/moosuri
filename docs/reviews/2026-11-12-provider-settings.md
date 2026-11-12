# Provider settings verification — 2026-11-12

This unit adds shared connection setup and per-user verified model selection. It
is not a live-model benchmark, conversation runner, subscription adapter or vault.
All accounts, keys, prompts and endpoints used for acceptance are synthetic.

## Evidence

- Rust1.89 workspace:70 tests pass on macOS and Linux x86_64. The new coverage is
  provider storage5, server integration3, catalog2; previous60 remain passing.
- `cargo fmt --all --check`, workspace/all-target Clippy with warnings denied pass.
- React29 behavioral tests, typecheck and production build pass. Exported Rust DTOs
  match the checked-in TypeScript contract.
- A dedicated real Rust server and synthetic loopback model were exercised through
  Chromium: sign in, restart with session retained, create compatible profile with
  no key, discover model, verify completed stream, set installation default, save a
  personal revision-pinned selection and reload. No production service was touched.
- Desktop/mobile CSS viewport inspection is recorded separately from real iOS or
  Android validation. User-owned and admin controls use the same RequestScope fencing.
- OpenSpec strict validation passes all10 specs/changes. Plan metadata keeps all28
  requirements,69 features and67 still-planned evaluation groups.
- Cargo.lock registry232 packages queried through OSV batch: no advisory or next-page
  response. This does not establish absence of every vulnerability.

## Failure boundaries

File-backed tests cover migration from schema1 without account/thread loss, key
permissions/symlinks/hardlinks/loss/substitution, authenticated ciphertext swapping
and tampering, credential destination changes, superseded ciphertext scrubbing,
admin/member separation, revoked sessions, isolated choices and disabled/stale
revision handling. Old selection pins cannot silently adopt a newly verified revision.

Real loopback HTTP tests cover catalog parsing/size/cancellation, synthetic probe
completion, missing/different model identity, truncated/incomplete streams and safe
upstream authentication failure. Disabling a connection during held response headers
ends local waiting and prevents a late result from activating the connection.

React tests cover catalog versus probe readiness, exact revision/model/effort payloads,
pending/failing saves, member access, changed connection state, explicit key replacement
or clearing, and stale administrator replies after account switching. No key value
is returned from provider GET endpoints or persisted in browser storage.

## Independent review and corrections

Separate reviewers inspected the HTTP/catalog and storage/crypto boundaries, then
the React/account boundary. Two material integration defects were corrected and
re-reviewed:

1. Completed probes originally ignored the reported model. Missing or different
   model identity now stays unverified; alias-to-snapshot prefixes are not guessed.
2. Creating a compatible profile without a key incorrectly sent `Keep`, which the
   create API correctly rejects. New blank keys now send `Clear`; existing blank-key
   edits preserve `Keep`. The mocked UI expectation was corrected and the actual
   browser/API creation path was verified.

Main integration additionally required explicit provider revisions in selection
requests to reject stale screens, and logical deletion of superseded ciphertext
rather than retaining every historical API key. Reviews found no remaining material
issue within the reviewed scope. Reviewers inspected source/fixtures; the main ran
the integrated suites. These reviews are not a formal cryptographic audit.

## Limits

Hosted live credentials, provider alias mapping, subscription authentication, model
quality/latency and actual messaging remain untested/unimplemented here. Compatible
servers may ignore undocumented settings; non-null effort is not offered for them.
A synthetic probe may consume provider usage, with a2,048-token/60-second cap.
Installed API keys are server-managed; an OS owner with both key and DB can decrypt
them. User-key vault secrecy is a distinct future boundary. Logical key deletion
cannot erase old snapshots/WAL pages. Backup/recovery and production migration are
still unavailable. Linux tests ran in an isolated limited container, not deployment.
