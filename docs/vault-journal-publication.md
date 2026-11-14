# Optional off-host vault journal publication

The separate vault service can periodically publish its full ciphertext journal
to an independently operated receiver over pinned OpenSSH. It captures actual
DB/WAL and the complete journal under the exact HTTP Database mutex, then releases
that writer before semantic verification, encryption or SSH. The source remains
live and is never sealed. No unlock secret, plaintext item, backup identity,
recovery identity, new OAuth scope or web export route is accepted.

This is opt-in periodic recovery coverage. The default interval is30seconds after
an attempt finishes; transfer duration and target failure extend the gap. Ordinary
writes do not wait for the receiver. An unavailable target has no bounded recovery
point objective, and effects after its last accepted checkpoint are not recovered.
This is not zero-loss deletion recovery or independent-world latestness.

## Initialize the independent target

Create a normal [live encrypted snapshot](vault-ongoing-backups.md) and retain its
exact head through a trusted handoff. Copy the selected snapshot/head to the target
through an operator-chosen trusted channel. Keep its matching age identity and the
journal recipient's age identity offline on a separate recovery device. Snapshot
and journal recipients may differ. Neither live source nor receiver needs those
private identities. The receiver pins this baseline and one public journal
recipient; rotation/rebasing is an explicit future target, not an automatic rewrite.

On the receiver prepare empty private directories and initialize:

```sh
umask 077
mkdir -m 700 /absolute/private/vault-artifacts
mkdir -m 700 /absolute/private/vault-ledger
moosuri-vault-service recovery target-init --target-id vault-1 \
  --artifacts /absolute/private/vault-artifacts \
  --ledger /absolute/private/vault-ledger \
  --snapshot /absolute/private/copied-snapshot.age \
  --snapshot-head /absolute/private/copied-snapshot.json \
  --recipient age1PUBLIC_JOURNAL_RECIPIENT
```

The target config/lock lives under `~/.moosuri-vault-recovery/vault-1`. Target IDs
and source SSH aliases are1..64 ASCII letters/digits/hyphen/underscore, beginning
with a letter or digit. Setup refuses populated, overlapping, linked or nonprivate
roots and copies/hash-checks the selected snapshot. Interrupted initialization can
leave incomplete new files; it is not silently adopted or cleaned up. Use another
explicit empty target after operator inspection. No source database is opened or
created by receiver commands, and no remote filesystem path comes from source.

Configure an existing SSH alias with a pinned host key and noninteractive
credentials. Restrict its source authorization to this exact forced command:

```sh
moosuri-vault-service recovery receive-journal --target-id vault-1
```

Use a dedicated receiver OS account with forwarding, PTY and general shell access
disabled. The source key must grant publication only, not arbitrary artifact/ledger
write/delete access or independent recovery access. The receiver and trusted
operator/storage account remain security boundaries. This tool does not create
accounts, edit SSH config/authorized_keys, configure infrastructure or prove that
a mount is off-host. Retain independent access credentials without the source/vault.

## Enable the source worker

Add these flags to the already configured vault-service invocation:

```sh
moosuri-vault-service --data-dir /absolute/private/vault \
  --origin https://vault.example.test \
  --assistant-origin https://assistant.example.test \
  --adapter-key-file /absolute/private/adapter-key \
  --journal-directory /absolute/private/vault-publication-stage \
  --journal-recipient age1PUBLIC_JOURNAL_RECIPIENT \
  --journal-snapshot-head /absolute/private/selected-snapshot.json \
  --journal-ssh-target backup-host --journal-target-id vault-1 \
  --journal-interval-seconds 30
```

The existing private0700 staging directory must be separate from the vault root
and its ancestors. All five journal settings are required together; interval is30
to604800seconds, default30. Local routine snapshot flags remain independent.
Only explicit journal settings start this sequential worker. It attempts once
immediately and sleeps after completion; no model/adapter calls are made by it.

Capture reuses live snapshot's DB/head checks, SQLite Backup, private staging and
an independent read descriptor for the full journal. It never clones/seeks the
live append descriptor. After releasing the writer it verifies actual captured
semantics; the source then sends the exact captured transcript over authenticated
encrypted SSH. Its private captured DB/journal stage is removed on completion;
hard kills can leave private source staging residue, as with local snapshots.
Only the recipient and selected baseline head are needed on source.

The SSH argv is fixed (`BatchMode=yes`, `StrictHostKeyChecking=yes`,10second connect
and5second keepalive bounds, cleared forwarding, no PTY). The source accepts no
arbitrary remote command, shell fragment, endpoint or remote path. Bounded pipe IO
and a60second transport deadline include cancellation/process-group kill and reap.
The60second deadline and cancellation apply only after SSH transport starts.
Source preparation (capture, journal scan/replay, table comparisons and digesting)
is file-size bounded but has no overall elapsed-time limit or cancellation
guarantee. The existing capture has deadline checks between some operations;
those checks do not bound the complete preparation. Graceful shutdown waits for
admitted preparation to drain, so it can wait for an extended period before any
transport starts. Cancellable/deadline-checked preparation remains a future
operational improvement. Once SSH starts, shutdown cancels and reaps that transport.

Receiver streaming separately checks its60second processing deadline; the receiver
CLI has a65second process watchdog for stalled stdin. OS disk stalls and fsync
are not hard real-time guarantees. Hard termination is separate.

Fixed status logs report `not_protected`, `accepted_at_checkpoint` or `lagging`.
Success includes only generation/sequence/exact trusted head hash and an explicit
periodic-gap label. Source ACK status is transient reporting, not independent
recovery authority. Restart recaptures; the receiver reestablishes prefix truth.
No raw journal, SSH stderr, path, key or private body is logged.

## Receiver validation and encrypted retention

The SSH framing is distinct `MVJP0001`. The receiver pins source UUID, the exact
baseline head hash and its configured public recipient. While streaming the full
journal it validates source prefix, typed bounded intents, every hash-chain step,
complete final sequence/head and full journal SHA256. It independently checks the
selected baseline boundary. Later acceptance also hashes the **exact incoming
first previously accepted byte count** and compares its SHA256 with the prior full
journal digest. A claimed newer sequence without the exact old-byte prefix is
rejected. Older/forked/foreign/truncated/extra-tail journals and changed recipient
or baseline reject. The source first verifies captured DB/journal semantics; the
receiver validates chain/prefix, and historical restore independently replays
actual semantics before destination publication.

The receiver streams straight into the existing historical `MVJC0001` age archive.
It writes no raw journal validation proofs or plaintext journal staging. Each
accepted generation contains only `journal.age` and `head.json`; the immutable
ledger holds bounded public sequence/hash/recipient metadata and links the prior
receipt hash. No decryption key is generated or retained on receiver. Ciphertext
journal entries are still sensitive; use private0700/0600 paths and protect the
receiver account/storage. Encryption does not hide these bounded receipt metadata.

A generation is accepted only after ciphertext/head files and their directory and
artifact parent are fsynced, then its no-overwrite ledger receipt is atomically
published and fsynced. Target-status and re-ACK verify current ciphertext/head and
receipt hashes and resync durability. ACK loss returns the **same checkpoint and
receipt** when the captured content is unchanged, even if source generated a new
attempt UUID. Old/forked input cannot replace it. Pre-receipt interrupted encrypted
generations are retained as orphans and are not current. Missing/corrupt current
artifacts, gaps or invalid ledger entries fail closed. There is no overwrite,
retention/GC or compaction command;100000 accepted generations is a hard cap.
Monitor private storage growth; exhaustion stops publication, not vault writes.

## Independently query and recover

Use a separately trusted receiver connection, rather than the source's reported
ACK or a downloaded adjacent head:

```sh
moosuri-vault-service recovery target-status --target-id vault-1
```

The bounded JSON returns `current:null` before first acceptance. Otherwise it
includes the exact `trusted_head_sha256`, full accepted checkpoint receipt and
retained snapshot/snapshot-head/journal/journal-head paths. Copy those selected
ciphertext/head files and use existing [historical restore](vault-historical-recovery.md)
with that independently queried journal-head pin and the matching offline
snapshot/journal identities. No source-local config or private recovery key is
needed. Existing restore exact-prefix/semantic checks, fresh service UUID, imported
session removal, pending-operation cancellation, nonce/receipt/deletion retention
and exact rerun preservation remain unchanged.

The current receiver checkpoint is its latest accepted local chain boundary, not
a proof of all source writes or external-world freshness. Post-checkpoint deletion,
rotation and nonce use are not reconstructed. Rolling back the entire receiver
ledger/artifacts and every independently retained pin can hide later acceptance;
retain trusted pins separately or use versioned/append-only target storage. Source
and restored copies remain independently runnable; this is not singleton/anti-fork
execution control. No production target, provider, real device or hardware
durability acceptance is implied by synthetic verification.
