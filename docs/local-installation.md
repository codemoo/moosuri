# Local signed preview bundles

The Rust installer accepts a **local signed directory bundle** and an explicitly
pinned Ed25519 public key. It does not download a release or invent an official
release key. Official signed artifacts, clean Linux x86_64/arm64 acceptance and
stable P10 release remain separate gates. The server runs with its React assets
embedded; Node/Python are not runtime dependencies. The separate vault and optional
broker are not installed by this command.

Choose two disjoint private absolute paths outside Git/cloud-synced/shared storage:
`/home/operator/.local/share/moosuri-runtime` for managed executables/config examples,
and `/home/operator/.local/share/moosuri-data` for private SQLite, journals and keys.
The runtime prefix must be new; existing directories, symlink components, traversal
and path control characters are refused. Opened private runtime/trust files and
directories also reject explicit/inherited ACL entries. Root/current-user ancestry
allows only trusted POSIX modes (sticky temporary roots are supported) and, on
macOS, deny-only ACLs; grant ACLs and Linux extended/default ACLs reject. The
installer never strips an operator’s ACLs. Bundle directory and public-key file are
owner-private; the pin is64hex characters representing exactly32public-key bytes.
Obtain/verify that pin independently of the bundle being installed.

```sh
moosuri installation install \
  --bundle /absolute/private/bundle \
  --trust-key /absolute/private/release-public-pin \
  --prefix /home/operator/.local/share/moosuri-runtime \
  --data-dir /home/operator/.local/share/moosuri-data \
  --origin https://assistant.example.test --network https

/home/operator/.local/share/moosuri-runtime/manager installation run \
  --prefix /home/operator/.local/share/moosuri-runtime -- \
  init --name Administrator --email admin@example.test
```

`init` reuses the existing hidden terminal input. A blank password generates a
one-time temporary password only on the controlling TTY. Unattended init accepts
an owner-only `--password-file`; no password is passed in argv or printed to logs.
Installation itself creates no account or model/Google connection. Init creates
only the administrator; use the web settings later. Data is not initialized until
that explicit init command.

For ordinary HTTPS/Nginx, `manager installation run --prefix PREFIX -- serve
--origin EXACT_HTTPS_ORIGIN --trusted-proxy 127.0.0.1` starts the selected verified binary with the fixed data
root. The manager retains shared runtime admission across exec; direct managed
release execution also respects activation admission. The existing server holds
its SQLite writer lock, binds loopback and drains on SIGTERM. Use `--dev` only for
an explicit loopback HTTP origin during a synthetic/local check.

## Review configuration and start explicitly

The prefix contains review files under `deployment/`:

- `moosuri.service`: Linux **user** service sample with the exact prefix/origin,
  private umask, bounded shutdown and restart-on-failure.
- `proxy.nginx.conf`: a location snippet for an operator-selected existing TLS
  virtual host; preserve Host, overwrite the client IP and disable SSE buffering.
- `tailscale-serve.txt`: optional tailnet-only direct Serve command and exact app
  origin. `--network tailscale` generates a service without `--trusted-proxy`; it
  does not provision Tailscale.

For direct Tailscale Serve, the generated Tailscale-mode service deliberately
ignores forwarded client-address headers. It accepts the actual loopback peer;
login attempt buckets use that peer plus the normalized account email. This
avoids treating a possibly chained `X-Forwarded-For` value as the single IP the
server parser requires. It does not claim per-tailnet-client IP attribution.
Do not add `--trusted-proxy` or point this topology at the HTTPS/Nginx-only header
contract. The HTTPS mode instead requires the supplied Nginx snippet to overwrite
one `X-Real-IP` address. Both modes retain normal application account login.

Review these files before copying/enabling a user unit or changing your chosen
proxy. Configure TLS/certificates and bind the exact origin yourself. No service,
DNS, vhost, Tailscale Serve or Funnel command is executed. Serve is not Funnel and
app account authentication remains required. Do not overwrite another service's
configuration. Unit installation/enabling, actual Tailscale/domain setup and the
separate vault origin remain operator actions.

## Stopped update and predecessor-schema migration

Stop the user service and wait for shutdown. Configure the existing encrypted
backup recipient with an independently held identity using `backup keygen/setup`
([backup](backup.md)). Then use a new local bundle signed by the **same pinned key**:

```sh
/absolute/private/new-bundle/moosuri installation update \
  --prefix /absolute/private/runtime \
  --bundle /absolute/private/new-bundle \
  --trust-key /absolute/private/release-public-pin \
  --backup-output /absolute/private/exports/before-update.age
moosuri installation status --prefix /absolute/private/runtime
```

The release sequence must increase and SemVer cannot decrease. Target, exact
binary size/hash, signature and self-reported build metadata/embedded assets must
agree. The selected old release's signed schema must match the stopped database.
The updater supports its own schema and **only the immediately preceding schema**;
it rejects a newer database, a schema gap, a mismatched old manifest or a target
schema different from its own build. Use the new bundle's executable for a schema
upgrade. Its executing bytes must equal the newly staged, pinned signed executable
before a source export or migration can proceed. Independently verify its public
pin as described above; no official release or automatic download is implied.

A running managed process or another data writer rejects the update. The original
stopped writer lock remains held continuously across source integrity/applied
journal verification, encrypted SQLite/journal export, product Store migration and
activation. An unconfigured backup or pending journal rejects before migration.
The export records the actual source schema. Export limitations remain as described
in [backup](backup.md); this is not a full filesystem backup or a promise that an
arbitrary historical export can be restored. Retain existing private files and
backup identities along with the source export.

Before migration, a fsynced `pending.json` preserves the old/new signed releases,
source/target schemas and export ID/path. A second durable migration-start marker
precedes opening the real Store migration path. No migration SQL is duplicated in
the installer. Existing same-schema updates retain their binary-only behavior.
Cross-schema activation also atomically refreshes the installed `manager` from the
verified new binary, preserving the service's existing entry point; deployment
files are not executed. A pending record fences managed startup until migration,
manager refresh, active-pointer publication and record retirement complete.

The command never starts/resumes the service. After status succeeds, explicitly
start the service and check `/health/ready` and `/app`; a failed start is not
silently called an accepted update. Immutable previous release directories remain
available. No database snapshot is automatically copied back or downgraded.

### Recover an interrupted update

Preserve the new signed bundle, source export and identities. If update reports an
error or the process stops with `pending.json` retained, keep the service stopped
and use the **same new signed bundle executable**:

```sh
/absolute/private/new-bundle/moosuri installation status \
  --prefix /absolute/private/runtime
/absolute/private/new-bundle/moosuri installation recover \
  --prefix /absolute/private/runtime --choose new
```

If interruption occurred before `pending.json` was published, no migration has
started. Repeat update with a fresh `--backup-output` path and retain any earlier
export; existing export files are never overwritten.

This is the recovery entry point even if the installed old `manager` rejects the
new schema/metadata. An interruption before manager refresh leaves that old
manager in place; an interruption after refresh permits the new installed manager
to recover too. Recovery verifies both pinned signed slots/current selection,
reacquires stopped data admission and retries the real Store migration
idempotently. It refreshes the manager and completes activation. A migration
failure keeps the pending fence and source export; resolve the reported cause and
retry `--choose new`. Unreadable/corrupt data remains a manual recovery case; do
not force startup by deleting metadata or journal files.

For a same-schema pending activation, `--choose old` still selects the old binary
without changing SQLite. For a cross-schema pending activation, choosing old is
permitted only before the durable migration-start marker and while the captured
database still has the exact source schema. Once migration has started it is
refused conservatively, even if the first transaction failed. New work or
authority committed after migration is preserved by retry; recovery never restores
an earlier database, replays an old backup or automatically downgrades a schema.

An interrupted first install before its marker was published remains a partial
new prefix; inspect it and choose a fresh prefix rather than recursively deleting
an arbitrary directory.

## Data-preserving uninstall

First stop/disable the user service you explicitly installed and remove only its
own proxy/Tailscale configuration if desired. Then:

```sh
moosuri installation uninstall --prefix /absolute/private/runtime
```

The command requires stopped managed/data writers. It removes only exact unchanged
recorded runtime files and verified signed release files. Modified configuration,
unrecognized entries and the private data directory remain. It never recursively
deletes a home/data/runtime directory and does not remove backups, identities,
system units or network configuration. Its output identifies retained changed or
unrelated entries. Deleting private data is a separate explicitly scoped operation;
there is no automatic data-erasure flag here.

## Packager metadata and external signing

Build React and the native binary from the pinned public source/lock first. Run
`moosuri installation probe` for public product/version/target/schema/embedded-web
metadata. Copy the resulting binary into a new private directory as `moosuri`, then:

```sh
moosuri installation manifest --binary /absolute/private/bundle/moosuri \
  --sequence 1 --output /absolute/private/bundle/manifest.json
```

Sign **the exact manifest bytes** externally with your independently controlled
Ed25519 key, producing the raw64-byte `manifest.ed25519` beside it. For an operator's
own key, OpenSSL's `pkeyutl -sign -rawin -inkey PRIVATE_KEY -in manifest.json
-out manifest.ed25519` is one packaging option. Export/verify its raw32-byte public
key through an independent channel and encode it as64hex characters in the private
pin file. Never place a signing private key in the bundle, source or server data.
These instructions do not establish an official project release key.

Format1 permits exactly one binary basename, `moosuri`; assets are embedded in it.
The signed JSON records product, native target, SemVer, monotonically increasing
sequence, exact Store schema, embedded-web flag, size and SHA256. Signature verifies
before parsing paths or running staged code; the installed copy is hashed again
and its public probe must match. The manager, immutable signed slots and review
config files live only under the owner-private prefix. No archive extraction,
install hooks, arbitrary post-install command or package-manager dependency is used.
