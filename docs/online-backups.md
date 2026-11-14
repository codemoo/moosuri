# Full-data backups while the assistant runs

After the separate recovery recipient has been configured with
[`backup setup`](backup.md), the optional live snapshot worker can capture full
SQLite data without stopping the service:

```sh
mkdir -m 700 /absolute/private/exports
moosuri --data-dir /absolute/private/data serve \
  --origin https://assistant.example.test \
  --backup-directory /absolute/private/exports \
  --backup-interval-seconds 3600
```

Only the operator configures this worker. There is no web route, user confirmation
or LLM call. The default interval is3600seconds, measured after each attempt
finishes; allowed intervals are300..604800seconds. A first snapshot is attempted
at startup. Only one attempt runs at a time; missed intervals are not replayed.
Missing recipient configuration or an unsafe directory rejects startup. A later
snapshot error leaves the assistant running and is retried after the interval.

Each successful export is a new private `assistant-UUID.age` file. Nothing is
overwritten or automatically deleted. Logs contain a fixed status, opaque export
ID and key epoch, never a path, private source text or key. Storage usage and
retention are the operator's responsibility. Keep the recovery identities outside
this directory and preserve the old identities needed by retained exports.

The existing single SQLite writer copies committed database/WAL pages and the
three journals into one immutable private stage. Journal heads must be fully
applied; an inconsistent or pending boundary is rejected. The capture briefly
occupies the writer; size limits and a60second checked deadline apply, but OS IO
stalls have no hard wall-time guarantee. No network, encryption or model work
runs on the writer. The subsequent verification, Push-authority stripping,
encryption and output publication use the frozen stage on a blocking IO worker.
Normal writes continue during that phase. The current implementation uses up to
two temporary database copies to reuse the stopped export's validation and codec.

The output is the existing format2 `assistant-export`, with all supported images,
files, memories and documents stored in SQLite. Provider keys, vault keys and
browser profiles are excluded. It can be used with the existing independent
[journal receiver](live-journal-publication.md) and
[historical restore](historical-restore.md). The receiver remains bound to its
explicitly retained baseline; creating a newer local export does not switch it.
Initialize a separately retained target for a different baseline when required.

These are local checkpoints. A snapshot does not protect changes made after its
capture, prove independent latestness or establish off-host retention. Historical
restore still requires a separately trusted later journal head and matching
identities, and quarantines old sessions, connections and work. The source is not
sealed. Keep the existing recovery worker for independent journal publication;
it retains its documented periodic recovery-point gap.

Orderly shutdown drains the active bounded attempt and starts no additional one.
An abrupt process or power failure may leave an owner-private staging directory
or partial ciphertext; a completed output is published only after encryption and
fsync using no-overwrite publication. Normal errors remove owned temporary state.
No secure-erasure, automatic off-host transfer or zero-loss guarantee is added.

[Acceptance evidence](reviews/2026-11-15-online-full-backups.md).
