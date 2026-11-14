# Manual historical export recovery

The development CLI restores one retained format2 assistant export using a later,
complete encrypted journal checkpoint. Publication briefly requires a stopped
source and leaves it active afterward. This is a manual recovery building block:
there is no live/unattended publisher, off-host transfer, remote freshness service,
or independent vault-service historical restore.

Configure and retain the separate age identity using [backup setup](backup.md).
Keep archives, head files and the independent latest-head record outside the
installation and retain copies outside its failure domain. A local private file
or a timestamp alone does not prove off-host retention or freshness.

After later changes, stop the source and publish journals for the chosen export:

```sh
moosuri --data-dir /absolute/private/source backup journal \
  --export /absolute/private/archives/retained.age \
  --output /absolute/private/journals/latest.age \
  --head-output /absolute/private/heads/latest.json
```

The command prints a SHA256 of the complete head. Retain that digest through a
separate trusted channel as the current recovery record. Each output is new and
must not exist. Ciphertext and head parents must be separate private directories.
The publisher retains the writer lock through publication, refuses pending journal
effects and never seals or changes the source. It encrypts full auth epochs, frozen
knowledge predecessor and destructive successor journals using the configured
public recipient. It never needs a private identity or the source provider key.
The head binds the exact chosen export ciphertext as well as the journal ciphertext,
manifest and complete applied heads. This authenticates the operator's selected
export once the head digest is trusted; the publisher does not decrypt or verify
that retained export's origin. Restore checks its origin and lineage before use.

A failed head publication can leave a complete orphan ciphertext. It does not
return a trusted head digest or advertise recovery completion. Retain the prior
trusted record and retry with fresh output paths; do not overwrite a newer record
with an old result. Source operation may resume after the command exits. Changes
after the captured checkpoint are outside its recovery coverage.

Recover into an absent destination under an existing private parent:

```sh
moosuri --data-dir /absolute/private/recovered backup restore-historical \
  --export /absolute/private/archives/retained.age \
  --journal /absolute/private/journals/latest.age \
  --trusted-head /absolute/private/heads/latest.json \
  --latest-head-sha256 INDEPENDENTLY_RETAINED_CURRENT_DIGEST \
  --export-identity-file /absolute/private/keys/old-identity \
  --journal-identity-file /absolute/private/keys/current-identity
```

Use the same identity file twice when both inputs use the same epoch. Rotation
does not make a new key decrypt old bytes: both required identities must survive
outside the original host and vault. Missing keys, wrong keys, mismatched digest,
missing/tampered ciphertext and non-extending journal prefixes fail before
publication. No plaintext or vault-key fallback exists. All inputs are retained.
A populated destination is accepted only when its immutable recovery provenance
exactly matches the fully verified inputs and trust pin. Such a retry only syncs
publication and reports the current account list; it never repeats quarantine or
rewinds enrollment or work. A different destination provenance is rejected.

The caller must supply the actual latest trusted digest. An older head is rejected
against a newer digest, but an operator-supplied stale head *and stale digest* cannot
be detected by these local files. Age recipients do not authenticate a writer;
trust comes from the separately retained digest of the head that binds both
ciphertexts. Copying an untrusted head next to an archive creates no such trust.

Restore authenticates age EOF before parsing bounded fixed tar inventories, checks
all entry hashes, installation IDs, SQLite integrity/FKs, frozen predecessor binding
and exact byte-prefix extension of all three journals. Supported format2 exports
begin at schema20 and migrate forward using the existing explicit restore ladder.
An unapplied legacy predecessor suffix is rejected. The journal checkpoint must use a supported schema20 through this binary's current
schema, be at least as new as the export schema and contain complete applied heads. The DB
stages privately, migrations precede suffix replay, and only the fully verified,
quarantined new installation is published by no-overwrite rename and fsync.

Journal-supported withdrawals apply before access: memory/source/collection
withdrawal, source revoke, direct forget target, artifact/media/mission/thread delete,
mail/calendar disconnect and batched source deletion. A post-snapshot UUID target
absent from that snapshot has no restored bytes. Historical revision drift is
handled conservatively: forget removes all snapshot versions of that target,
artifact delete removes its snapshot versions, and disconnect retires the snapshot
connection without requiring intermediate edits. This may remove more old history
than ordinary live replay. It does not reconstruct later additions, corrections,
usage or successful external work absent from the export. Non-journaled removals,
such as community package uninstall, are not claimed to be reconstructed. Learned
proofs and automatic work authority are quarantined regardless.

All accounts require explicit local enrollment with a new credential. Historical
roles become `member`; local `recover-account` explicitly selects a current role.
Opaque auth epochs do not reconstruct latest role or suspension state. The operator
must enroll only currently eligible accounts with their current role. Snapshot
suspended status remains suspended; later deletions remain deleted and their
private tenant rows are purged before publication. Other tenants retain their
snapshot content. Authentication is never opened merely because journal replay
succeeded. Old sessions, credentials, providers, source connections, browser control,
Push, schedules, missions, learned proofs and incomplete external work use the same
restore quarantine as [sealed restore](restore.md). Failed/pending receipts remain
historical evidence and external actions are not automatically resent.

Historical accounting is explicitly `accounting_unknown` in immutable restore
provenance. Snapshot receipts and reservations remain a lower bound; unexported
charges are not zero. This development path is **data recovery only**: every shared
or personal model binding/probe (including STT, vision and automatic memory) and
current usage-total view stays unavailable even after new credentials and account
enrollment. The guard conservatively includes local/free configurations. There is
no new-allocation or accounting reconcile flow yet, and no CLI bypass. A later
sealed restore cannot erase the historical accounting gap because its immutable
provenance remains present. The original source installation is unaffected.

The source remains active-capable. Before enrolling the destination, stop the
original deployment or otherwise ensure a single active service. This manual
restore does not constitute cutover, operator account recovery, publication of a
backup outside the host, or an ongoing disaster-recovery guarantee.
