# Manually pinned vault journal recovery

An older [live snapshot](vault-ongoing-backups.md) can be recovered through a
separately selected encrypted **full journal checkpoint** from the same source.
Recovery requires an independently retained SHA256 of the exact later receipt and
an exact snapshot journal prefix. It preserves journaled effects through that
selected head, including deletion, owner epoch revocation, key/root rotation,
receipts and nonce reservations. It does not infer latestness, off-host durability
or effects committed after that checkpoint.

## Export a journal checkpoint

The manual CLI requires the normal stopped writer lock. Stop the service, export
to new files under an existing private0700 directory separate from its data root,
then explicitly resume it if desired. Source admission is not sealed or retired:

```sh
moosuri-vault-service --data-dir /absolute/private/vault \
  recovery journal-checkpoint --recipient age1PUBLIC_RECIPIENT \
  --output /absolute/private/kit/newer-journal.age \
  --trusted-head /absolute/private/kit/newer-journal-head.json
```

The command prints `vault_journal_head_sha256=HEX`. Retain that exact pin through a
separately trusted handoff, independently of replaceable archive/head storage.
An adjacent head file, public-recipient encryption and hash chaining alone do not
authenticate source authorship or prove current/latest history. The operator
selects the trusted source checkpoint; filename order grants no authority.

Export captures actual DB/WAL and full journal consistently and verifies complete
applied head and semantic replay. Only the fixed journal entry is encrypted; the
private DB copy is cleaned up. `CapturedSnapshot::encrypt_journal` can also consume
a capture taken under the existing live service mutex, encrypting after release.
This unit adds no HTTP export endpoint or periodic journal receiver/transport and
does not change the existing snapshot worker. Healthy source DB/journal evidence
must agree. Missing files, poisoned/general unapplied crash gaps are not repaired,
truncated or guessed, and absent sources are not initialized.

The distinct `MVJC0001` / `moosuri-vault-journal-checkpoint` format binds source and
checkpoint UUIDs, sequence/head, exact full journal size/SHA256 and ciphertext
size/SHA256. Bounds remain1GiB journal,16KiB manifest/head and32MiB per intent plus
bounded framing/age overhead. Publication retains private files, fsync and
no-overwrite rules. Failed receipt publication can leave an orphan ciphertext;
that is not a successful trusted pair. No key or unlock secret is generated/read.

## Recover through the independently selected head

Select an absent destination under a private0700 parent. Retain all evidence and
the matching backup identities outside it:

```sh
moosuri-vault-service --data-dir /absolute/private/recovered-vault \
  recovery restore-historical \
  --snapshot /absolute/private/kit/older-snapshot.age \
  --snapshot-head /absolute/private/kit/older-snapshot-head.json \
  --snapshot-identity-file /absolute/private/kit/snapshot-identity.txt \
  --journal /absolute/private/kit/newer-journal.age \
  --journal-head /absolute/private/kit/newer-journal-head.json \
  --journal-head-sha256 INDEPENDENTLY_RETAINED_64_HEX_PIN \
  --journal-identity-file /absolute/private/kit/journal-identity.txt
```

The two outer backup recipients can differ. Snapshot head selection retains its
existing separately trusted operator handoff contract. No vault master/recovery
secret is accepted. Only live snapshots are supported as historical bases; original
sealed-current and plain live-snapshot restore formats remain unchanged.

Both age streams authenticate through EOF and exact file/metadata checks. The
later receipt must match the independent pin, same source UUID and a sequence at
least as high as the snapshot. Its full journal must start with the **exact old
journal bytes**, source prefix and captured sequence/head. Foreign sources,
older/divergent chains, truncation, bad pins and tampered ciphertext reject before
destination publication.

Private staging then replays later Prepare/Adopt/Cancel/Lifecycle/Close through the
actual storage rules. Every generated sequence/head and final complete journal
bytes must reproduce the selected transcript. CAS/header/inventory/nonce checks
remain; a hash-valid pinned impossible adoption rejects. Close carries only a
closed-session digest. Later ciphertext may require non-authoritative owner
namespace anchors; unjournaled session-only state never becomes permission. Normal
service open retains its previous strict general crash-tail rejection.

Before publication every latest pending operation is cancelled using its exact
body/reservations and all imported sessions are removed. Earlier and later
deletions, receipts, ciphertext/header history and nonce fences survive. A fresh
service UUID and distinct historical provenance bind both selected inputs. Fresh
assistant grants and independently configured adapter key are required. Exact
rerun preserves subsequent destination work; different pins, plain restore mode,
unrelated occupied roots and reserved staging destinations cannot adopt/replace
it. No database rollback or source authority change occurs.

There is no automatic off-host receiver, independently current pin, remote
monotonic coordinator or zero-loss guarantee. An old valid pin selects old
coverage; post-pin deletion/nonce use cannot be reconstructed. Source/restored
copies may accept fresh grants independently: anti-fork/singleton enforcement is
separate. Retain original encrypted evidence and identities under the existing
private retention contract. [Synthetic evidence](reviews/2026-11-15-vault-historical.md)
does not establish production, real-device or hardware durability acceptance.
