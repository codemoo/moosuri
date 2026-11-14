# Optional live journal publication to an independent receiver

This preview captures complete auth/deletion journals while the assistant runs and
periodically sends them to a separately operated Rust CLI receiver through OpenSSH.
It retains the existing historical restore format, separate age recovery identities
and quarantine. It does not seal the source or make ordinary writes wait for a
backup. The default publication interval is30seconds **after each attempt ends**;
transfer duration and target failures extend the recovery point gap. This is not
zero-loss deletion recovery, and an unavailable target has no bounded RPO.

## Prepare the independent target

Make a stopped [encrypted baseline export](backup.md), note the installation UUID
printed by `backup setup`, and copy that ciphertext to the receiver through an
operator-chosen trusted path. Keep every required old/new age private identity on
an independent recovery device. Neither source live worker nor receiver requires
those private identities. The baseline copy is not a database migration/cutover.

On the receiver, create separate empty private directories and initialize:

```sh
umask 077
mkdir -m 700 /absolute/private/recovery-artifacts
mkdir -m 700 /absolute/private/recovery-ledger
moosuri backup target-init --target-id assistant-1 \
  --installation-id INSTALLATION_UUID \
  --export /absolute/private/copied-baseline.age \
  --artifacts /absolute/private/recovery-artifacts \
  --ledger /absolute/private/recovery-ledger
```

Target IDs are1..64ASCII letters/digits/hyphen/underscore, starting with a letter
or digit. The CLI manages config/lock under `~/.moosuri-recovery/assistant-1`.
It refuses populated roots, symlinks, nonprivate directories, overlapping roots
and known source/package directories. Baseline bytes are copied and hash/size
verified. Initialization never overwrites an existing target; an interrupted first
setup may leave incomplete newly created target files and must be examined by its
operator before trying a fresh target ID/empty directories. No automatic cleanup
of an existing target is provided.

Configure an existing OpenSSH alias with a pinned host key, noninteractive
credentials and receiver access. The source uses the fixed remote command:

```sh
moosuri backup receive-journal --target-id assistant-1
```

Restrict the source SSH authorization to that forced command, disable forwarding
and PTY, and run it under the dedicated receiver OS user. Source needs publication
access, not a general shell or arbitrary write/delete access to the receiver's
artifacts or ledger. SSH configuration and independent recovery access are the
operator's deployment responsibilities; this CLI does not modify authorized_keys,
create accounts or install a remote service. The target and ledger must be retained
outside the source's failure domain. Two directories on the source disk do not
constitute off-host retention. Keep receiver access credentials recoverable without
the original assistant host or vault.

## Enable the source worker

Add these optional flags to the existing serve command:

```sh
moosuri --data-dir /absolute/private/source serve \
  --origin https://assistant.example.test \
  --recovery-export /absolute/private/archives/baseline.age \
  --recovery-ssh-target backup-host \
  --recovery-target-id assistant-1
```

The SSH alias uses the same restricted character grammar as target ID. No remote
path or arbitrary command string is accepted. The worker uses BatchMode and strict
host-key checking, connection/keepalive timeouts, bounded pipe IO and a60second
transport deadline. Shutdown cancels and reaps its transport process group. Failure
diagnostics do not copy SSH stderr, journal bytes, keys or private source data into
ordinary logs. The source hashes the baseline once at worker start; the target
requires that exact already retained baseline for every acceptance.

Startup reports `not_protected`. Each completed attempt reports only public
checkpoint/receipt metadata: `accepted_at_checkpoint`, `lagging`, or `not_protected`,
with the last ACK when available. Acceptance means the reported captured boundary,
not that subsequent writes are protected. This in-memory status is not authority
and is lost on restart; restart republishes and target prefix checks establish
truth independently. Target-status is the authoritative operator query below.
No web configuration or web status screen is included in this unit.

## Capture, trust and ordering

Capture is one job on the existing SQLite writer. It checks active installation,
verified recipient and complete applied auth/predecessor/successor heads, then
copies the three bounded journals into a private immutable temporary stage. No
append can interleave within that job. Encryption and transfer happen after the
writer is released. File caps and checked deadlines bound admission; an OS IO stall
has no hard wall-time guarantee. Maximum journal size remains64MiB each and
capture/receiver processing uses the existing60second checked deadline.

Only opaque UUID/epoch/destructive metadata travels on authenticated encrypted
SSH. The receiver is explicitly trusted to read this metadata. It verifies the
existing journal codecs and exact old-byte-prefix extension under its own lock,
then encrypts the exact validated bytes with the source's verified public age
recipient. It has no private age identity. For prefix validation the receiver
retains the raw opaque journal proofs in private0600 files inside each generation.
These proof files are **not age-encrypted at rest**: protect the dedicated account,
its storage and volume encryption accordingly. Original source text, attachments,
provider tokens and user vault secrets are not present in these journals.

Receiver creates immutable numbered generation directories with age ciphertext,
head and validation proofs. It fsyncs that generation and its artifact parent
before atomically publishing/fsyncing the next independent ledger receipt. The
receipt links the prior receipt digest and binds head digest, captured-content
fingerprint, ciphertext and checkpoint. Only that receipt makes a generation
accepted/latest. A source must receive its matching fingerprint ACK before reporting
acceptance. Source time, snapshot metadata and downloaded head alone never prove
independent latestness. Unchanged journals/schema/key epoch return the existing
receipt without advancing generation. Public-recipient rotation advances a new
checkpoint while retaining the old baseline and required old key identity.

Interrupted pre-receipt generations are not latest and may be retained under an
orphan name. A receipt committed before ACK loss is recognized on retry, synced
and returned unchanged. Older or forked prefixes and recipient/schema rollback
are rejected. Complete metadata/hash disagreement or a missing current artifact
fails closed. The target keeps at most100000accepted generations; it currently has
no retention/compaction/GC command. Raw journal, archive and ledger growth must be
monitored; limit exhaustion stops publication, not ordinary assistant writes.

The receiver operator/storage administrator remains trusted. Rolling back all
copies of its ledger, artifact state and independently held receipt digest can
hide a newer checkpoint; no local timestamp or hash chain proves external-world
freshness after rollback of every trust anchor. Independently retain the current
receipt/head digest or use append-only/versioned receiver storage. Distinct ledger
and artifact directories allow separate retention, but the CLI cannot prove a
mount's host/failure-domain independence.

## Recover without the source

On the independently accessed receiver:

```sh
moosuri backup target-status --target-id assistant-1
```

The bounded JSON reports the current accepted receipt (including
`trusted_head_sha256`) and retained export/journal/head paths. Obtain these through
a separately trusted recovery connection, copy the three ciphertext/head inputs,
and use [historical restore](historical-restore.md) with that digest and all matching
old/new offline identities. No source-local config or private key is needed.
An empty target status is `current:null`, not protected. Downloading an untrusted
head beside ciphertext is not a replacement for independently querying the current
ledger. Existing restored account/credential/work quarantine and historical
accounting/new-budget restrictions still apply.

The latest accepted receiver checkpoint may predate source writes at failure.
Data/deletions after that checkpoint are not reconstructed. A future strict mode
would need remote intent durability before acknowledging every auth/deletion
operation and would fence those writes during target outages. This periodic worker
does not implement that mode, vault-service recovery or zero-loss latest-deletion
disaster recovery. Optional [live full-DB snapshots](online-backups.md) now produce
local exports separately; they do not switch this receiver's pinned baseline. No production target is configured or
tested merely by installing this preview.
