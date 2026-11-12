# Account and empty-thread storage foundation

`Store::open(data_dir)` creates a single SQLite writer thread and a bounded
64-command mailbox. All connections remain private. Mailbox saturation and the
global two-job password KDF limit return `StoreError::Busy`; KDF work runs on
Tokio's blocking pool with its permit held until completion, including when a
caller disconnects. The supported boundary is a single Unix host and a local
filesystem with working file locks and fsync. No provider is called.

The process holds `writer.lock` until the last `Store` clone drains and closes.
The selected directory is owner-only (0700), files are owner-only (0600), and
symlink path components, symlink files and multiply linked files are rejected.
Pass an actual path rather than a system symlink alias (for example use
`/private/tmp` instead of `/tmp` on macOS). These permissions protect other OS
users; they do not isolate arbitrary code running as the same UID. SQLite uses
bundled SQLite3.54+ (rusqlite0.41 / libsqlite3-sys0.39+, Rust1.89 minimum), WAL, foreign keys, FULL synchronization and a two-second busy
timeout. Startup checks schema version, SQLite integrity, foreign keys and the
authentication journal before accepting work.

Accounts and personal tenants have independent opaque UUIDs. All empty-thread
operations revalidate the session, account epoch and tenant. Admin permissions
allow listing/creating accounts and never bypass thread tenancy. Account email
normalization is ASCII trim + lowercase, preserving dots and plus suffixes.
Passwords accept 12–256 UTF-8 bytes, use random-salted Argon2id (19 MiB, two
iterations, one lane), and are never stored as plaintext. Session tokens are 32
random bytes encoded as lowercase hex; the DB stores SHA-256 digests only.
Regular sessions expire after 30 days and successful authentication renews
activity. Temporary credentials expire after 24 hours and create restricted
sessions until an atomic password exchange consumes all previous sessions and
credentials. Returned bearer tokens and invitations have no `Debug` or serde
implementation. Transient password inputs are zeroized inside this crate.

## Password revocation journal

Before changing a password, the actor appends and fsyncs a binary record to
`auth-epochs.journal`. A record contains only its format, sequence, opaque
account UUID, new epoch, previous digest and SHA-256 integrity checksum. The
password hash, account update, session revocation, new session and journal
watermark then commit in one SQLite transaction. A failure after append poisons
the current actor, so it refuses further work until reopened.

At startup, a journal ahead of the DB advances the account epoch, removes its
sessions and sets `recovery_required`. Its old password hash remains unusable
for new logins. The foundation intentionally has no recovery/unlock API: a
crash in this narrow gap requires a future explicit recovery workflow. A
partial record, invalid checksum, missing required record or mismatched DB
watermark prevents startup. The checksum chain detects damage and truncation
against the DB watermark; it is not an authenticated defense against a malicious
OS owner replacing both journal and DB. This local journal has no independent
off-host latest-head/backup service and is not a backup or restore implementation.

## Explicitly outside this unit

There are no messages, model runs, streams, files, provider bindings, vault,
account disable/delete/reset/role-change APIs, operator recovery, backup/restore,
independent usage ledger or restore activation. Deleting a thread deletes only
an empty-thread row. Future personal child objects must use composite tenant
foreign keys. Rate limits, cookies, CSRF/origin checks and HTTP setup protection
belong to the server layer. Authentication does not unlock a vault.

Tests use dedicated synthetic filesystem DBs. They exercise tenant crossing
(including admin), composite foreign keys, restricted-session access, concurrent
temporary-password exchange, credential expiry, revocation, reopen persistence,
subprocess writer exclusion, private paths and journal crash/truncation gaps.

```sh
cargo test --locked -p moosuri-store
cargo clippy --locked -p moosuri-store --all-targets -- -D warnings
```

The SQLite version floor is checked before opening a DB/WAL pair. This excludes
versions affected by [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html);
package lock/advisory lookup does not establish absence of every vulnerability.
