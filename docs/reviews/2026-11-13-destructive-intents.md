# Direct destructive intent coverage — 2026-11-13

Scope: direct destructive intents and explicit export format2. The integrated
version applies automatic-memory0014 before successor migration0015; the shared
supported schema is15 and the coverage baseline remains15.
No dependency/lockfile change, new crypto, live provider call, restore/import,
checkpoint, off-host publication, activation or deployment is included.

## Covered boundaries

- A pending legacy knowledge prefix is replayed before successor effects; its exact
  bytes freeze. New operations append only MSDSTR01, retaining MSAUTH01 unchanged.
- Migration interruption keeps a valid private96-byte installation/predecessor
  header verbatim. Invalid partial, missing installed, wrong-installation,
  changed-predecessor and truncated successor inputs fail closed without reset.
- Thread-delete and mail-disconnect TEMP triggers abort the real effect transaction
  after actual journal append/fsync. Readiness and later destructive mutations fail
  until reopen; replay purges the intended scoped data and retains the other tenant.
  Repeated disconnected-mail requests do not append another record.
- Existing knowledge/command derived-purge, auth revocation, source/timeline/FTS,
  target-plan and receipt assertions remain. New command records directly contain
  memory UUID, command UUID and pinned head/effective revisions. A separate saved
  SQLite snapshot predates its slot, proving the direct mapping can address the
  memory without discovering it through that later slot. This is a component
  proof; normal replay still requires and validates its sealed receipt slot.
- One mixed page includes two withdrawals, an update and a new source. A TEMP
  trigger fails after its single DeleteSources record fsync. Replay withdraws both
  while page upserts, receipt, sequence, checkpoint and page count remain uncommitted.
  Old lease retry is fenced. The same business UUID/page content/checkpoints with
  a fresh server lease commits once, exact subsequent ACK retries are stable,
  no second deletion record is appended and withdrawn identities cannot revive.
- A literal schema13 format1 manifest round-trips byte-for-byte through its
  original type and produces exactly four encrypted tar entries. Format2 has
  five entries, required version1/baseline15 coverage, exact successor byte hash/
  size and complete/applied heads. Pending successor adoption is exported without
  replay or source mutation. Missing coverage/journal, bad heads and format
  downgrade fail. Existing WAL/private-file/lock/rejection bounds remain.

## Fixture adaptations

Empty successor journals contain a96-byte bound header rather than zero legacy
bytes. Three existing zero-append checks now assert that exact header length.
Legacy schema3/4 migration and missing-legacy-journal tests retain their original
MSKNOW01 paths and zero-byte expectations.

Two existing command tests synthesized a dead-process gap but then deleted its
chat through the live public API before reopen. Thread deletion is now itself
journaled, so that arrangement would append a second operation beyond an unapplied
first one. Those fixtures directly arrange the same scoped thread purge/DB deletion
while retaining their receipt/replay assertions. The actual public boundary is
separately covered: after a real post-fsync SQL failure the actor returns
Unavailable for another public destructive mutation until replay. No runtime
selector behavior was changed to make the fixtures pass.

## Integrated verification and review

macOS focused journal7, automatic-memory12, knowledge17, command32 and backup10
cases pass (filters overlap); the reference archive test is separately selected.
CLI backup boundaries2, native keygen->setup->format2 export->reference Go age
decryption1, and Rust/Go interoperability1 pass. fmt, all-target warnings-denied
clippy, plan and strict OpenSpec checks pass.

The first Linux run across Store/Agent/Server/CLI passed273 tests and failed one
format1 compatibility fixture;4 explicit model/network/reference cases were
ignored. The fixture created empty input files with platform-default0644 mode.
The production reader correctly required private inputs. The fixture now uses
explicit0600 file creation and sync, retaining its exact literal format1 bytes
and four-entry assertions. The targeted Linux case and remaining crate doc-tests
then pass. The initial failure is retained; these results are not described as a
single clean full-suite run. No production permission check was relaxed.

Independent review found a retry durability gap: a complete header/record left
readable after failed fsync could be adopted on reopen without another successful
sync. Validated journal opens now fsync before returning authority, and reused
headers also sync their directory. Injected header and record sync failures prove
that schema/effect/watermark cannot advance while sync fails; successful reopen
reuses the exact bytes. The fix and integrated startup/replay order, native keygen,
private publication, automatic-memory suppression/withdrawal hooks and explicit
format2 boundary were independently rechecked with no remaining concrete defect.

Earlier isolated fixture issues and their corrected runs remain historical evidence;
no behavioral assertions were relaxed to make integration pass. Linux checks used
verified read-only source/vendor mounts in the pinned nonroot/no-network Rust1.89
image, without production service or data changes.

Coverage begins at0015 and cannot reconstruct earlier unjournaled thread/mail
omissions. No independent latestness or historical restoration is claimed. Logical
deletion does not erase old WAL pages or snapshots, and a checksum chain does not
authenticate a writer that can replace the entire installation.
