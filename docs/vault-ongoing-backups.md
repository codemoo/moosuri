# Routine encrypted vault snapshots

The separate vault service can take opt-in encrypted point-in-time snapshots while
it continues serving. This path never writes `vault.sealed`, retires the original
service, changes sessions or needs a user unlock secret. The existing
[sealed-current checkpoint](vault-service-recovery.md) remains a separate stopped
cutover contract with its unchanged format and required `--seal-source` flag.

Prepare a private0700 output directory separate from the vault data root and its
ancestors. Keep an existing X25519 age identity independently, outside the service;
configure only its public recipient. For example, add these flags to the existing
explicit service invocation:

```sh
moosuri-vault-service --data-dir /absolute/private/vault \
  --origin https://vault.example.test \
  --assistant-origin https://assistant.example.test \
  --adapter-key-file /absolute/private/adapter-key \
  --backup-directory /absolute/private/vault-snapshots \
  --backup-recipient age1PUBLIC_RECIPIENT --backup-interval-seconds 3600
```

Both backup directory and recipient are required together. The interval is300 to
604800seconds, default3600, measured after the previous attempt finishes. The worker
attempts one snapshot immediately, then runs sequentially; idle waiting makes no
model or adapter calls. It neither creates keys nor installs services or network
configuration. Existing vault web build/origin/adapter requirements still apply.
There is no HTTP export endpoint. A separate opt-in
[periodic journal publisher](vault-journal-publication.md) can retain later journal
checkpoints on an independently configured SSH receiver; initial snapshot transfer
remains an explicit operator task.

The worker shares the exact mutex used by HTTP ciphertext storage. Under that
writer exclusion it checks DB/head identity, copies committed WAL through SQLite
Backup and copies the complete bounded journal through an independent read
descriptor. It never seeks/clones the live append descriptor. Capture retains the
source installation UUID and sequence/head. Encryption, full journal semantics
replay/integrity checks and publication use only the frozen private stage after
releasing the service writer. Requests needing the writer can wait during capture;
the service remains running and encryption does not occupy that mutex. Capture
checks a60second deadline between bounded operations; blocking IO/fsync/replay is
not a hard real-time guarantee.

Successful attempts publish a new `vault-snapshot-UUID.age` and matching
`vault-snapshot-UUID.head.json`, both0600 with no-overwrite publication and fsync.
The head pins exact ciphertext size/SHA256, source/checkpoint UUID and captured
journal sequence/head. The distinct `moosuri-vault-live-snapshot` purpose and
`MVLS0001` framing and `snapshot` receipt/manifest field identify a non-sealing
snapshot. The original `seal` receipt is not synthesized. Neither public-recipient encryption nor an adjacent head proves
authorship, independent latestness or off-host durability. Retain the selected head
through a separately trusted operator handoff.

Failures never seal or modify source authority. A failed head publication can
leave a complete orphan ciphertext without its trusted receipt. Do not treat that
as a successful pair. Normal cleanup removes only the attempt's private stage;
kills can leave private temporary residue. No automatic retention/deletion policy
or off-host copy is provided. The operator retains paired archives/heads and the
independent identity. Graceful SIGINT/SIGTERM drains an admitted capture/encryption
before worker exit; hard kills and physical power-loss behavior remain distinct.

## Explicit point-in-time restore

The destination must initially be absent under an existing private0700 parent.
Use the separately selected trusted head and identity, outside that destination:

```sh
moosuri-vault-service --data-dir /absolute/private/restored-vault \
  recovery restore-snapshot \
  --checkpoint /absolute/private/kit/vault-snapshot-UUID.age \
  --trusted-head /absolute/private/kit/vault-snapshot-UUID.head.json \
  --identity-file /absolute/private/kit/backup-identity.txt
```

`recovery restore` accepts only the existing sealed-current mode;
`restore-snapshot` accepts only the new non-sealing mode. Both use authenticated
age EOF, exact file/hash/head checks and actual journal semantic replay. Before
publication, snapshot restore cancels every retained pending operation using its
exact body/reservations and removes every imported session. Retained ciphertext,
history, nonce reservations, receipts and deletions already present at capture
remain checked. A fresh service-local UUID and immutable restore provenance bind
the new root. New assistant authentication/grants and independently configured
adapter key are required; no old session/pending command is activated. Repeating
the exact completed restore only resyncs that matching root and preserves new work.
An unrelated occupied root or reserved staging destination rejects.

Plain `restore-snapshot` does **not** attach a newer journal. The separate manual
[pinned journal recovery](vault-historical-recovery.md) can recover through an
explicit full journal checkpoint with exact-prefix and semantic checks.
An older snapshot by itself cannot know about item/owner
deletion or nonce use committed after its captured head. Explicitly restoring it
can therefore recover older ciphertext; it must not be called latest, deletion-safe
through a later date, or zero-loss. Choose/retain snapshots according to their
actual capture coverage. The manual historical path requires its distinct encrypted
format and independent head hash; no live source journal is silently attached and
no latest head is inferred. The optional periodic receiver can supply its latest
accepted head pin through an independent connection. Periodic gaps, actual target
setup and changes after the last accepted checkpoint remain outside that coverage.

The original service stays live, and restored copies have distinct service UUIDs.
They can accept fresh grants independently; there is no distributed singleton or
anti-fork coordinator. This is a routine backup/explicit recovery tool, not a
cutover claim. Bounds and private path/owner/fsync rules follow the existing
recovery contract:512MiB DB,1GiB journal,16KiB manifest/head and32MiB per operation.
The service never receives the backup identity, vault master password or plaintext
items. Synthetic [verification evidence](reviews/2026-11-15-vault-ongoing.md) does
not establish production deployment, real device or hardware durability acceptance.
