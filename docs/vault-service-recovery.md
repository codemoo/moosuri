# Standalone vault sealed-current recovery

The separate ciphertext service supports an **offline, destructive cutover of its
exact current checkpoint**. Stop the service first. This is distinct from the
[user-local encrypted item archive](vault-archive.md), the assistant's backup,
and general disaster recovery. Historical rollback, an independently current
off-host journal, unattended backup, old-v1 migration, account deletion/outbox,
credential broker and independent signed-client distribution remain separate.
No user master password, recovery key, decrypted item or assistant adapter key is
needed or exported.

Keep a standard X25519 age identity in a separate owner-only0600 file, with its
public `age1...` recipient. An identity made by `age-keygen` or the assistant's
`moosuri backup keygen` works; this service does not generate or retain it.
Use an existing private0700 export directory outside the source root:

```sh
moosuri-vault-service --data-dir /absolute/private/vault recovery checkpoint \
  --recipient age1PUBLIC_RECIPIENT \
  --output /absolute/private/kit/vault-current.age \
  --trusted-head /absolute/private/kit/vault-current-head.json \
  --seal-source
```

`--seal-source` is required. After validating the stopped writer lock, complete
journal/DB identity, the command creates and fsyncs
`vault.sealed`, then fsyncs the source directory **before copying source bytes**.
Every ordinary service/Database open refuses a sealed root. A crash or output
failure after sealing deliberately leaves it retired; it is never automatically
unsealed. Retry checkpoint creation with fresh output paths; it reuses and
validates the exact durable seal. A partial/corrupt seal fails closed. The source
journal is never truncated, repaired, replay-adopted or rewritten to close a gap.
Keep the old sealed source until the new root is verified.

The SQLite Backup API includes committed WAL pages. The complete private snapshot
and exact journal are encrypted with age0.11.5 X25519 using the existing library.
The version1 binary plaintext framing has a fixed magic, bounded manifest and
two fixed length-delimited files, SQLite followed by journal; it contains no
caller-selected paths or tar entries. The manifest binds source/checkpoint UUID,
schema1 journal sequence/head and exact file byte sizes/SHA256. Header/item
ciphertext, historical receipts, deleted-item tombstones, nonce reservations and
encrypted pending candidate bodies are retained. Outer encryption also protects
metadata and session digests inside the snapshot. Unlock secrets, private backup
identity, adapter key, browser files and other arbitrary source files are excluded.

Output is a new0600 file, fsynced then published by atomic no-overwrite hard link
and parent fsync. The separate owner-private trusted head pins its exact
ciphertext hash/size and seal identity. Retain that head through an independently
trusted handoff. Public-recipient encryption and a head saved beside an archive
prove neither authorship nor latest/off-host history. A malicious operator who
can replace the head or delete the source seal is outside this local cutover
fence; no remote rollback resistance is claimed.

## Restore into an absent private root

Retain ciphertext, trusted head and its matching identity outside the destination.
Its parent must already be owner-private0700; the destination must be absent:

```sh
moosuri-vault-service --data-dir /absolute/private/new-vault recovery restore \
  --checkpoint /absolute/private/kit/vault-current.age \
  --trusted-head /absolute/private/kit/vault-current-head.json \
  --identity-file /absolute/private/kit/backup-identity.txt
```

Restore validates exact ciphertext digest, full age authentication through EOF,
fixed framing/bounds and every file hash. It rejects missing, ahead, truncated or
mismatched journal/DB evidence, unsupported schema or unexpected SQLite schema.
It runs SQLite integrity/FK checks, then recomputes every Prepare/Adopt/Cancel
through the actual storage CAS/nonce APIs and compares every ciphertext/history,
operation receipt/status and nonce reservation row. Session-close journal entries
are checked by the journal chain but carry no restored execution authority.
Owners without ciphertext remain non-authoritative namespace anchors. Semantic replay uses private ciphertext staging after the source seal; a detected semantic inconsistency therefore leaves that source retired and preserves its original evidence.

Before publication it removes **all sessions** and cancels **every pending
operation** using that operation's exact retained body/nonce reservation. These
cancellations append normal durable journal intents. Committed, aborted and
conflict receipts, immutable history and deletion tombstones remain. The source
stays sealed and unchanged. Fresh assistant authentication/handoff/online commit
is required for future work; no imported session or pending command can execute.
The service stores no broker pairing/grants in this schema and exports no adapter
key. Configure the adapter key independently when starting the restored service.

A fresh service-local installation UUID is written consistently into the new DB
and journal prefix. The retained journal intent transcript keeps its hash chain;
immutable `vault.restored` provenance binds this new identity to the exact old
checkpoint/head. Vault owner/tenant/vault AAD and encrypted wrappers remain
unchanged; the assistant installation UUID inside future fresh grants remains
that assistant's identity. Copies restored to different destinations each get
unique service UUIDs. They can each accept newly issued assistant grants: there is
no distributed singleton coordinator or prevention of deliberate checkpoint
forking. Source retirement prevents the honest original service from continuing
with its old identity; it does not establish global freshness.

The staging root is owner-private and ordinary service admission rejects its
reserved `.vault-recovery-stage-*` name. Files and staging directory are fsynced;
native Linux/macOS no-overwrite directory rename publishes the new root, followed
by parent fsync. Other platforms fail closed. Every database open syncs its parent
before admission. A late publication/fsync failure leaves a complete retryable
root. Repeat the exact restore: immutable provenance and new DB identity select
that completed destination, resync it and preserve all subsequent sessions/work;
quarantine is not applied twice. An occupied unrelated root is never overwritten.
Inputs remain untouched. Normal failures attempt to remove only the attempt's
private staging root; a kill/cleanup failure can leave private residue. This is
not secure erasure and retries do not adopt/delete other residue.

## Limits and verification scope

Only local filesystems with functioning locks/fsync are supported. Bounds are
512MiB SQLite/WAL input/snapshot,32MiB SHM,1GiB journal,16KiB JSON manifest/head and their
combined encrypted framing bound. Individual intents retain the existing32MiB
operation bound. There is no compression, passphrase/SSH/plugin recipient,
arbitrary-file extraction or plaintext-export fallback. IO/fsync/replay can block;
there is no hard real-time completion guarantee. Export/restore require the
current OS owner and private regular single-link inputs, reject symlinks in all
path components, refuse overwrite and never initialize missing source evidence.
Every existing path entry must be owned by root or the current UID. Directory
ancestors with group/world write are rejected unless sticky; each next existing
entry is also root/current-UID owned, so another UID cannot rename it through a
trusted sticky parent. This applies to ordinary service admission as well as
recovery source, input, output, destination and provenance paths. Canonical
root-owned sticky `/private/tmp` (macOS) and `/tmp` (Linux) remain usable with
current-UID private children. A root/current-UID operator can still change its own
path permissions or entries; that malicious-operator case remains excluded.
Restore rejects reserved `.vault-recovery-stage-*` destination basenames before
reading/decrypting checkpoint data, so a successful restore cannot publish an
ordinary-service-inadmissible staging name.

Synthetic tests exercise exact restart/restore, session and pending quarantine,
receipt/reservation/tombstone preservation, copied distinct service identities,
repeated restore preserving new work, active lock, linked/public-mode inputs,
occupied output/destination, journal gaps and semantic corruption, wrong identity,
ciphertext/head corruption, truncation and limits. Native test results and any
remaining independent/Linux acceptance belong in the integration evidence; these
local tests do not establish production recovery, real-user migration or the full
vault roadmap.

The repaired source passed native and isolated Linux all-target service tests
(lib2, CLI1, HTTP1, recovery7, storage4) and Clippy with `-D warnings`. Independent
storage/security review found and verified repairs for mutable ancestor paths and
reserved restore names. Actual Linux UID1000/1001 probes confirmed foreign-owned
path rejection and root-owned sticky-parent protection. Fault tests inject errors
after the durable seal and before/after publication; they do not establish physical
power-loss or hardware-fsync behavior. See the [integrated acceptance record](reviews/2026-11-13-vault-service-recovery.md).
No real account, running service, private vault or production data was touched.
