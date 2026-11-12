# Private knowledge verification — 2026-11-12

This unit adds manual private text sources and explicit memory management. It does
not connect knowledge to the chat model, extract memories automatically, fetch
external documents or claim memory-quality evaluation results.

## Executed checks

| Surface | Result |
| --- | --- |
| Rust 1.89, macOS | 111 passed: Store 38, runtime 12, connectors 45, HTTP 15, CLI 1 |
| Linux x86_64 | Same 111 passed in an isolated Rust container, read-only source, dedicated build/data paths |
| Final React assets | CLI installation test rerun on macOS and Linux after the UI fixes |
| Static checks | Rust fmt and workspace/all-target clippy with `-D warnings` passed |
| React | 65 behavior tests, TypeScript check and production build passed |
| Contract | Rust-generated TypeScript matches the checked-in DTOs |

The new checks include atomic batch/receipt/cursor writes, replay and stale cursor
rejection, tenant and administrator isolation, exact Unicode source evidence,
temporal corrections, bounded Korean search, source-dependent invalidation and
history removal, explicit forgetting, scoped collection keep/delete, journal crash
replay, migration interruption and missing/corrupt journal rejection. They use
synthetic file-backed databases. The existing tests were not copied from the legacy
application; old checks continue to protect the newly written foundation.

React checks cover uncertain write identities across navigation, conflicting edits
without draft loss, exact selected UTF-8 spans including repeated text/CRLF, stale
search/detail/account results, scoped draft cleanup, retained collection deletion
and deliberate abandonment after a reconciled collection-creation conflict.

## Independent review

A non-author reviewed Store/core/migration, then HTTP/CLI and the frozen React
slice. Four material findings were corrected and independently rechecked:

1. Source deletion could erase a temporal supersession barrier and expose an older
   assertion. Content-free revision/start-time barriers now survive dependent-text
   removal; three-revision and reopen regressions cover it.
2. Migration 4 could commit before its required journal existed durably. Journal
   file/directory sync now precedes schema commit; interrupted upgrades can retry.
3. Kept/disconnected collections had no later bulk-delete action. The retained
   collection now has an explicit deletion flow using its current revision.
4. A lost-ACK collection creation reconciled against a deleted/disconnected scope
   could permanently trap its draft. The UI permits deliberate abandonment after
   conflict while preserving the original identity during unknown ACK status.

Idempotent collection creation was also reviewed: the same active scope/name
returns the original collection before quota checks; conflicting names and retired
collections cannot mutate or revive it.

## Real browser and publication

Chromium used the actual Rust server and a dedicated synthetic database. The
journey covered collection creation, Korean paste/search, a precise evidence
selection after an emoji, memory save/correction/history, session/data persistence
across server restart, forgetting while retaining the original source, and
collection keep followed by deletion. The 390 × 844 viewport was visually inspected
with no horizontal document overflow and an independently scrolling work area.
Browser viewport checks are separate from
real iOS/Android validation.

The public source allowlist excludes local agent instructions, runtime data and
browser artifacts. Public source/history secret scans and strict OpenSpec/plan
validation are release gates. GitHub Actions remains disabled. Cargo/npm lockfiles
are unchanged; the 232 registry dependencies retain the earlier OSV-query evidence,
not a new assertion that all possible vulnerabilities are absent.

## Limits

Search is lexical and bounded, without embeddings. Manual saves are explicit
assertions or user-authored paraphrases with validated source spans; quote validity
does not prove the truth of a paraphrase. Suppression hashes are reserved for future
automatic extraction, not a semantic denylist for manual assertions. The deletion
journal is not supported backup/restore or forensic erasure of historical WAL or
filesystem snapshots. Ordinary knowledge is distinct from the future encrypted
user-key vault. No production collector, service, cron, DB or real model account was
used or modified.
