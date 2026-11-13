# Encrypted exports of a stopped installation

The development CLI can export its current SQLite data and snapshot journals to an
age-encrypted archive. It requires a stopped installation and an independently
held X25519 backup identity. **Historical restore, unattended backup, an off-host
latest journal and authoritative media recovery are not implemented.** Do not
move production accounts or data into this preview on the basis of export support.

## Prepare a separate recovery identity

The backup identity is different from an app password, provider key or future
user vault master/recovery key. Keep it outside the installation, Git, served
web directories and the export destination. Keep an independent copy accessible
without the original server or the vault being recovered.

Generate an identity on your own recovery device with the Rust CLI:

```sh
umask 077
mkdir -m 700 /absolute/private/recovery-kit
moosuri backup keygen --output /absolute/private/recovery-kit/assistant-backup-identity.txt
```

`backup keygen` prints only the public `age1...` recipient. The private identity
stays in a new owner-only0600 file after file and parent-directory fsync. It refuses
overwrite, linked/nonprivate directories and known source/installation directories.
It does not open or initialize a database, need `--data-dir`, or register the key.
Keep the recovery directory separate from any future installation/export directories;
paths are not retained or tracked across commands. A standard `age-keygen` identity
also works. No external age program is required by the application.

Stop moosuri, then register the public recipient and a separately retained copy
of the private identity:

```sh
moosuri --data-dir /absolute/private/data backup setup \
  --recipient age1PUBLIC_RECIPIENT \
  --identity-file /absolute/private/recovery-kit/assistant-backup-identity.txt
```

The command encrypts and decrypts a small synthetic challenge before activating
the recipient. Its identity file must be bounded, owner-only, a regular file
owned by the current OS user, with one hard link and no symlink. It accepts one
standard X25519 identity, with optional comment lines. No private key value
belongs in command arguments, logs or source. The identity and its path are never
stored in the database. Because no private path is retained, separation from a
later export directory is an operator responsibility, not a cross-command path
check. Fixed archive entries never include an identity file. A successful local challenge proves the supplied key can
decrypt; it does not prove off-host storage or a clean-host restore.

The installation must already be initialized and upgraded through the normal app
open path to the supported schema. Export and setup do not initialize a missing
installation or run migrations. Existing missing/corrupt installation metadata
or journals fail closed. Backup remains unconfigured until setup succeeds, and
ordinary app use does not require a backup identity.

## Create an export

Use an existing owner-private destination directory separate from both the data
root and the recovery kit. The command refuses source/Git/package directories,
linked path components, an existing output file and an active installation lock.

```sh
mkdir -m 700 /absolute/private/exports
moosuri --data-dir /absolute/private/data backup create \
  --output /absolute/private/exports/assistant-2026-11-13.age
```

The stopped writer lock remains held during capture and encryption. SQLite's
Backup API incorporates committed WAL pages; it never copies a live DB alone or
deletes WAL/SHM. Private temporary DB+WAL copies avoid read-only SQLite creating
sidecars beside rejected original inputs. The exporter never starts runtime
workers, decrypts provider credentials or needs `provider-secrets.key` to exist.
It streams four fixed standard tar entries inside a standard age-v1 ciphertext:

- `manifest.json`: purpose/version, installation/export IDs, public key epoch,
  snapshot schema, byte hashes/sizes and auth/knowledge journal heads/watermarks.
- `assistant.sqlite`: the complete current database snapshot, including retained
  public recipient history and existing tombstones/receipts.
- `auth-epochs.journal` and `knowledge-invalidations.journal`: exact snapshot-era
  journal bytes, checked against the snapshot's committed prefix watermarks.

The manifest purpose is `assistant-export`. Its authoritative-file inventory is
explicitly empty because attachment/photo/PDF/artifact storage is not implemented.
Provider/OAuth/connector ciphertext may remain in the snapshot, but the server
key, operator identity, external credential files, browser profiles/capsules and
user vault keys are excluded. Any future restore must purge/re-register server
credentials and quarantine old authority/work/accounting; this export does not
implement or bypass those gates.

The complete output is published without overwrite only after tar and age
finalization, file fsync and parent-directory fsync. It is owner-only0600.
Ciphertext/manifest hashes detect corruption; public-recipient encryption does
not authenticate the writer or prove journal freshness. Snapshot journals are
not an independently maintained latest recovery checkpoint. Later deletions
cannot be enforced by an older export without that separate future mechanism.

Only local filesystems with working file locks/fsync are supported.

Current bounds are1GiB per DB/WAL input and resulting snapshot,64MiB per journal,
64 retained key epochs,32KiB manifest and a60-second deadline checked between
snapshot/archive steps. Exceeding a bound fails explicitly; data is never silently
truncated. File IO/fsync can block at the OS level; this is not a hard real-time
shutdown guarantee. Normal/error returns attempt to remove private temporary copies and
partial outputs; cleanup IO failures or an abrupt process kill can leave owner-private temporary data
or a partial ciphertext for the operator to remove. This is not secure erasure.

## Rotate and retain keys

Generate/retain a new independent identity and run setup with its public recipient
and private identity file. Only after the challenge succeeds does one SQLite
transaction append a new public key epoch and activate it. The installation UUID
and older public epoch history remain durable. Repeating setup with the current
recipient verifies it again without creating an extra epoch. Missing or invalid
backup tables/metadata are not silently replaced with a new identity/epoch1.

Keep every old private identity needed by retained exports or future required
journal segments. A new key does not decrypt an older archive. Only verified
re-encryption or expiry of every dependent retained item permits old-key disposal.
The app persists no private identity, and cannot recover one that the operator
loses. This version does not offer an automatic retention/delete policy.

Error codes include `backup_unconfigured`, `backup_busy`, `backup_limit_exceeded`,
`backup_key_missing`, `backup_decryption_failed`, `backup_output_exists`,
`backup_unsupported_installation` and `backup_integrity_failed`. None falls back
to plaintext export or substitutes a vault master key.

## Implementation and dependency evidence

Exports use exact Rust `age0.11.5` with default features disabled and X25519
recipient APIs, `tar0.4.46` without xattr support, and rusqlite's Backup feature.
The published age library declares Rust1.65; the locked dependency graph builds
with the application Rust1.89 toolchain. Reference Go age1.3.1 decrypts an actual
application export, and Rust age decrypts reference-produced synthetic ciphertext.

The added dependency review reports two build-time maintenance advisories:
`basic-toml0.1.10` (RUSTSEC-2026-0374) and `proc-macro-error2 2.0.1`
(RUSTSEC-2026-0173), reached through age's localization proc macros. These parse
build metadata or produce compiler diagnostics, not runtime identity/archive
input. They remain tracked dependency limitations; this is not a zero-advisory
claim. The pin avoids introducing unnecessary alternate-recipient crypto
implementations. No SSH/plugin/passphrase or compression path is enabled.
