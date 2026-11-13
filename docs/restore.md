# Restore a sealed cutover checkpoint

This development command restores the exact full current-state checkpoint made by
[sealed checkpoint](checkpoints.md). The combined schema19/20 path passed independent
storage/auth review and integrated Linux checks; see the [evidence](reviews/2026-11-13-sealed-restore.md). It does not import ordinary
older exports, merge installations, unseal the source or provide ongoing recovery
freshness after a host loss.

Retain three owner-private input files outside the old and new installation: the
checkpoint ciphertext, its head received through a separately trusted channel,
and its matching age identity. A head beside a ciphertext is not independent
trust or proof that the sender supplied the latest checkpoint. The files stay
unchanged. Choose an absent destination under an existing owner-private parent:

```sh
moosuri --data-dir /absolute/private/new-data backup restore \
  --checkpoint /absolute/private/kit/cutover.age \
  --trusted-head /absolute/private/kit/cutover-head.json \
  --identity-file /absolute/private/kit/recovery-identity
```

Restore verifies pinned ciphertext and age authentication through EOF before
parsing a bounded fixed archive. It verifies every file hash, the sealed DB,
installation/seal/checkpoint identity, schema, frozen predecessor and complete
applied journal heads. An unsupported future or older export fails. Only explicit
supported forward migrations run in the private offline stage.

A new installation UUID and immutable restore provenance accompany quarantine.
The historical successor journal keeps its original byte-identical lineage; an
immutable restore event binds that lineage to the new installation. The source
stays permanently sealed. Existing content, withdrawal state, dependencies,
committed receipts and accounting survive. Files are private, fsynced and published
by native no-overwrite directory rename followed by parent-directory fsync.

Every account needs operator enrollment, including the original administrator.
All sessions are deleted and account epochs advance; roles and suspension stay
unchanged. The command prints opaque account IDs and roles. For each selected
account, use its printed role and a new owner-only password file:

```sh
moosuri --data-dir /absolute/private/new-data recover-account \
  --account-id <printed-id> --role admin \
  --password-file /absolute/private/kit/new-temporary-password
```

Then log in with that temporary password and perform the mandatory password change
within 24 hours. Configure backup setup again for the new installation: imported
recipient epochs are not an active recovery grant. Keep identities for old inputs.
Old passwords and sessions cannot log in. Operator recovery never
reactivates a suspended account; an active administrator must explicitly change
its status through account management if desired.

The old server provider key is intentionally absent from the checkpoint. Provider
metadata and old encrypted credential bytes can exist inside the encrypted input,
but usable provider credentials do not survive restore. Restore retires imported
credentials, disables model revisions, clears probes/selections and creates a new
private key. Re-enroll and verify model connections. Gmail/source credentials,
OAuth enrollment and source tokens must also be registered again. Browser control
is closed; source connections require reconnect; automatic extraction grants and
schedules stay disabled/paused even after account enrollment. Queued chat and
uncommitted mail are cancelled; already committed unknown external dispatch stays
unknown. No pending action is resent. Committed mail/model/source/notification
receipts and accounting remain historical evidence.

On failure, retry the same command with the same retained inputs. Each attempt
uses a new private reserved staging path that ordinary Store/raw entry points
reject. A killed attempt can leave private staging residue; retry neither adopts
nor deletes someone else's residue. A complete matching destination after a late
publication/fsync failure is recognized from immutable provenance and resynced.
Every runtime, operator-recovery and offline writer admission holds the exclusive
writer lock while syncing the destination parent before new authority or mutation.
A failed parent sync stops admission and leaves the complete root retryable.
A post-publication retry preserves subsequent destination enrollment and work state;
it does not apply quarantine a second time. An unrelated occupied destination is
never overwritten. There is no cleanup or
physical erasure claim for old inputs, exports or interrupted staging residue.
