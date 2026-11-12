# Durable knowledge agent acceptance — 2026-11-12

Scope: the new migration5 ledger, three read-only knowledge tools, two-round capability
proof, derived-output revocation and exact references in React. All observations use
synthetic accounts, sources and loopback model servers. No production collection,
credential or paid/hosted model was used. CI remains disabled.

## Independent review

A non-author reviewed the Store ledger: opaque leases, dispatch admission, checkpoint
CAS, complete-call slots, cumulative limits, actual/inherited provenance, invalidation
journal replay and late accounting. Another review covered runtime/HTTP integration;
a fresh UI reviewer covered reset/history/action races and exact evidence navigation.
Material findings fixed and re-reviewed:

1. Full resnapshot retained older cached turns after an offline missed reset. It now
   clears cached history before loading and fences late actions/older-page tasks by epoch.
2. Cumulative dispatch/tool rejection appeared as authority revocation. A typed budget
   error now yields `incomplete / budget_exhausted`, including retries and deadline expiry.
3. A well-formed5KiB search parameter exceeded the4KiB stored slot and aborted the run.
   The runtime now records a bounded invalid-argument result and permits model repair.

The Store intentionally uses a conservative generation fence: any knowledge change
can stop an in-flight run once it depends on knowledge. Completed unrelated output
survives. Dispatch admission precedes network IO; already-admitted requests or previously
sent bytes cannot be recalled. These are documented limits, not a claim of remote erasure.

## Automated evidence

- Rust154 tests: Store57, runtime18, connectors59, HTTP19 and CLI installation1.
  The separate child-process writer-lock invocation is part of Store57, not an extra test.
- New runtime tests drive the real supervisor/file DB/loopback HTTP through Korean
  search→exact read→final, persisted per-step receipts, malformed/5KiB argument repair,
  foreign-ID denial, deletion during held output, restart/retry budgets and atomic
  tool-batch overflow. Two independently authored tests reproduced the wrong budget
  outcome before the fix and passed after it.
- Actual HTTP regression covers completed evidence-backed output→source DELETE→stale
  SSE reset→fresh purged snapshot, then deletion while a later model response is held.
  Late bytes never restore the answer; accounting remains and no dispatch is replayed.
- Provider tests check wrong names/arguments/challenge/identity, incomplete rounds,
  missing text proof and revocation during the second capability round.
- React70 tests include immediate reset, offline missed reset, late retry response,
  exact revision navigation and rejecting a false capability response. TypeScript,
  generated Rust DTO comparison and production build pass.
- macOS and isolated Linux x86_64 Rust1.89 workspace tests, fmt and all-target
  clippy(`-D warnings`) pass on the same runtime/source and built React assets. The final deletion-scope copy is
  covered by React70 and rebuilt embedded-asset installation checks.
  Strict OpenSpec14 changes/specs pass; plan validation checks metadata and local links.

## Browser evidence

Chromium used the real embedded Rust app and a separate synthetic compatible server.
The administrator completed the two-round tool proof for an existing text-verified
local model. A new collection and Korean source with an emoji were created through
React. Asking about the saved meeting produced a completed multi-step answer and an
actual server-backed source reference. The390×844 viewport had document width390,
independent chat overflow:auto and composer bottom829; its screenshot was inspected.

Opening the answer reference showed the exact saved Unicode source. Deleting that
source through the UI and returning to chat removed the derived answer/reference,
showed the knowledge-invalidated state, and preserved the unsent composer draft. This viewport check is not actual iOS/Android,
a full accessibility audit or a model answer-quality/latency benchmark. Tool proof is
protocol interoperability evidence; it cannot verify a third party's internal behavior.

## Remaining work

Automatic extraction, conversational memory changes, semantic retrieval, missions,
schedules, external tools/skills, media and the user-key vault are separate work.
No full P0/P1/P3/P4 phase, live-model mission score or production cutover is claimed.
Registry dependencies stay at the previously checked232 packages; this unit only
changes the existing serde dependency placement in the agent crate. Ordinary knowledge
and private tool capsules live in the assistant DB and are not the user-key vault.
