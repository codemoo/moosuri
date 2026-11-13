# Sealed current-snapshot checkpoints (development only)

This command permanently retires a stopped source installation and produces one
self-contained age-encrypted cutover snapshot. It is for a deliberate migration
handoff. The matching [restore command](restore.md), independent storage/auth
review and integrated Linux checks are complete for this bounded path, including
schema21 image storage and forward migration from19/20. [Validation and limits](reviews/2026-11-13-sealed-restore.md) distinguish
synthetic acceptance from operating a real migration.

First configure and retain an independent recovery identity as described in
[encrypted exports](backup.md). While the installation is still active, settle
any ordinary startup recovery. Pending journal effects cause checkpoint admission
to fail before sealing; this command does not replay runtime work or decrypt the
server provider key. Ordinary application use still needs no backup setup.

Use existing private directories separate from the installation/source tree and
from each other for ciphertext and trusted-head output:

```sh
moosuri --data-dir /absolute/private/data backup checkpoint --seal-installation \
  --output /absolute/private/checkpoints/cutover.age \
  --head-output /absolute/private/recovery-kit/cutover-head.json
moosuri --data-dir /absolute/private/data backup checkpoint-status
```

The required flag authorizes permanent retirement, not a temporary pause. No
unseal, new checkpoint generation or recipient rotation exists. After the durable
seal, `serve`, `init`, `doctor`, account recovery, backup setup and ordinary exports
refuse admission. Direct library openers have the same fence. The checkpoint/status
paths never construct Store or start workers. Installation-independent backup
keygen and public DTO export remain available outside installation data.

The local state progresses active -> sealed -> checkpoint_ready. A FULL-sync DB
transaction adopting the seal is irreversible even if later output fails. It fixes
installation/seal/checkpoint IDs, public recipient epoch, schema, coverage and all
three journal heads/hashes. Complete heads must equal applied heads. The redundant
seal marker is an additional fence; a missing marker cannot bypass the DB state,
and unexpected controls alongside an active/older DB fail closed. New runtime
admission inspects DB plus committed WAL through a private temporary copy under
the same writer lock, before source chmod/create, migrations, replay or key handling.
Active legacy first-open behavior is retained after the read-only sealed check;
account recovery still requires its already-existing installation lock.

Once the encrypted stage is finalized and file/directory-synced, another FULL-sync
transaction stores its immutable size/hash and head receipt. Only then can output
publication begin. Retry returns the same IDs and ready ciphertext bytes; it cannot
re-encrypt a ready checkpoint. Missing or truncated exact staged ciphertext can be
repaired only from an already-published output with the receipt's exact hash/size;
conflicting stage bytes remain untouched and fail. Unexpected staging entries fail.

Ciphertext is published before the deterministic head. Both use private temp files,
file fsync, native Linux/macOS no-replace rename and parent-directory fsync. Existing
matching outputs are checked and re-synced; any different output conflicts without
overwrite. The complete destination can exist after a late sync error. Retry the
same command, optionally using new output paths to copy the same bytes. Cleanup
removes only this attempt's tracked matching inode. Process-kill partials are private
residue, preserved rather than recursively removed; no GC or secure erasure is claimed.

Checkpoint format1 has purpose `assistant-sealed-cutover` and exactly six regular
tar entries: manifest.json, assistant.sqlite, auth-epochs.journal, the frozen
knowledge-invalidations.journal, destructive-intents.journal and cutover-seal.json.
Manifest hashes/sizes, installation/seal IDs, public key epoch, capture schema and
coverage version1/baseline15 agree with the archived DB's sealed row. The snapshot
precedes the local ready-receipt update and contains the actual final user state.
Source updates can invalidate memory/index/derived data without a new destructive
journal head, so a journal suffix over an older export cannot replace this DB.
Integrated schema20 tests preserve all seven real artifact tables, withdrawal and
receipts, quarantine schedules, and retain account suspension.

Original export format1/four entries and format2/five entries remain unchanged.
An older export does not acquire historical replay eligibility from sharing these
journal heads. The external authoritative-file inventory remains empty: current
documents, canonical images and thumbnails are stored inside the SQLite snapshot.
Schema21 preserves ready image bytes and tombstones, quarantines unfinished uploads
and clears image capability proofs during restore. PDF and external blob storage
are not implemented. No dependent checkpoint/old identity chain
is needed to decrypt this self-contained package. Retained older archives still
require their own independently retained identities.

The separate head uses purpose `assistant-sealed-cutover-head`, closure kind
`sealed_installation_cutover_v1`, fixed IDs, schema/coverage and journal heads plus
manifest/seal/ciphertext hashes and size. It contains no private identity, credential,
user text or filesystem path. It is **not automatically trusted** because it sits
beside matching ciphertext. Carry that exact head through a separately trusted
operator channel and retain ciphertext/identity independently before abandoning
any source. This implementation transfers nothing off-host and verifies no transfer.
Age recipient encryption and hashes do not authenticate the publisher or prove
freshness; timestamps and an old recovery kit cannot prove latestness either.

Within the supported binary, local lock and fsync boundary, no operation can be
acknowledged by this source after its durable seal. This does not prove there are
no other live copies or defend against a malicious same-UID whole-root rollback.
Periodic checkpoints while continuing runtime are not ongoing disaster protection.
The implemented [restore](restore.md) consumes this exact final-state DB, quarantines
credentials and unfinished work, pauses schedules, preserves suspension/accounting
and requires fresh account enrollment. Historical archive replay remains unavailable.

Bounds:1GiB DB/WAL/snapshot,64MiB per journal,32KiB manifest,16KiB seal,64KiB receipt,
8KiB head and2GiB ciphertext. The60-second cooperative deadline is not an OS IO
or fsync timeout. Every failure remains explicit, with no truncation or plaintext
fallback. Private plaintext temporary capture cleanup is best effort after failures
or process death.
