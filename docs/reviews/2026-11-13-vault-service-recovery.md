# Standalone vault recovery acceptance — 2026-11-13

Accepted scope: stopped, permanently sealed **exact-current** vault-service cutover
into a new private root. This does not claim general historical disaster recovery,
remote freshness, broker integration, real-user migration or the complete vault roadmap.
The assistant schema and user vault crypto format remain unchanged.

## Resulting behavior

- The source lock, exact DB/journal identity and durable source seal precede any
  snapshot copy. Output failure leaves that source retired; an exact retry never
  unseals it or overwrites an existing output.
- Bounded fixed framing is encrypted with the existing age0.11.5 X25519 library.
  Restore authenticates the complete archive through EOF and checks exact hashes,
  SQLite integrity/FKs, identity and semantic CAS replay of journal history.
- Ciphertext history, receipts, deletion tombstones and nonce reservations survive.
  All sessions are cleared and pending operations are durably cancelled using the
  retained exact bodies. Fresh service identity and immutable provenance accompany
  fsynced no-overwrite publication. Exact repeated restore preserves later work.
- Existing path entries must be root/current-UID owned; writable nonsticky ancestry,
  symlinks, hard-linked private inputs and reserved staging destinations are rejected.
  Trusted root/current-owned sticky temporary ancestry remains usable.

## Independent findings and repairs

The initial frozen source review requested two repairs. VR-1 found that a mutable
foreign/nonsticky ancestor could be renamed between pathname checks, separating the
writer lock from later DB/seal IO. Shared ancestor validation now applies consistently
to ordinary runtime admission and every recovery path. VR-2 found a restore destination
with the reserved staging basename could be published but refused by normal runtime;
the shared predicate now rejects it before input reads or staging.

A second independent review accepted the repaired source conditionally on execution.
The root checked the repaired bytes, implementation, native/Linux results and actual
UID evidence before accepting this unit. Reviewed production bytes stayed unchanged.

## Execution evidence

Native and pinned Linux all-target tests passed: library2, CLI1, HTTP1, recovery7,
storage4. Both all-target Clippy runs passed with `-D warnings`. Tests cover exact
restore/restart and retry, quarantine, receipt/reservation/deletion preservation,
active locks, malformed/mismatched/truncated inputs, semantic replay corruption,
occupied destinations and injected interruption boundaries.

Linux used a read-only source snapshot, existing offline vendor/build cache, network
none, read-only container root, no-new-privileges and UID1000 with all capabilities
dropped. Separate synthetic setup changed selected ancestor ownership to UID1001;
product probes rejected those real foreign-owned paths while preserving source and
input bytes. An actual UID1001 process demonstrated rename authority under its own
nonsticky parent and denial under the root-owned sticky parent protecting UID1000
children. The trusted sticky positive completed checkpoint, restore and runtime open.

The first native attempt reused stale Calendar artifacts from the shared target and
failed compilation. Its errors and logs were retained. Refreshing source mtimes only
forced the exact frozen checkout to rebuild; all bytes remained identical and the
gates then passed. No cache deletion, source repair or dependency replacement was used.

The original and repaired source freezes, native logs, independent reports and Linux
UID probes are retained as private synthetic development evidence. No operational DB,
real account or credential was used. Public source and exact commit scans, formatting,
plan links and OpenSpec validation form the publication checks.

## Limits

The separately trusted head selects one exact archive; it does not establish authorship
or independently current history. Deliberate copied-checkpoint forks and a malicious
root/current OS operator remain outside this local cutover fence. No physical power-loss,
hardware-fsync, off-host journal, old-v1 migration, device or production gate is claimed.
See [the operational contract](../vault-service-recovery.md) before using the destructive
source-sealing command.
