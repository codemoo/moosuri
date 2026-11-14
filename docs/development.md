# Run the development preview

Build and run the current development preview with the commands below. This is not
an installer or a stable release. [Current status](status.md) is the canonical map
of supported behavior, validation limits and remaining work. Every email send
requires explicit approval of its final preview.

Use Rust 1.89+ and Node.js 22.22+. Node is needed to build React, not to run the
resulting server. SQLite is bundled. Build from a clean public checkout:

```sh
npm ci --ignore-scripts --prefix web
npm run build --prefix web
cargo build --locked -p moosuri
```

Choose a private absolute data directory outside Git, cloud-synced folders and
network filesystems. On Linux the default is `$XDG_DATA_HOME/moosuri` or
`~/.local/share/moosuri`; macOS development defaults to the user Application
Support directory. `--data-dir` or `MOOSURI_DATA_DIR` overrides it. The directory
must be private and empty or an existing moosuri installation; unrelated entries
and symlinks are rejected. Do not point it at your home or source root.

```sh
./target/debug/moosuri init --name 'Administrator' --email 'admin@example.test'
./target/debug/moosuri serve --origin http://127.0.0.1:3210 --dev
```

The hidden terminal prompt accepts a password of 12–256 UTF-8 bytes. Leaving it
blank generates a temporary password shown once on the controlling terminal;
first login must change it. For unattended setup, `--password-file` reads a
small owner-only regular file and never prints its contents. No password value
is accepted as a command argument. Only the administrator is created initially.

Open `http://127.0.0.1:3210/app`. An administrator can use `/admin` to create
additional members with one-time local invitation credentials. No invitation
email is sent. Each member must change their temporary password before using
personal or admin APIs. Open
`/admin/providers` for [shared connections](provider-settings.md) and
`/settings/model` to choose a verified model/effort for your account. Send a question
in `/app`; [text chat](chat.md) explains streaming, retry/cancel and recovery.
`/settings/usage` shows observed usage; `/admin/usage` shows account counters.
Use `/app/knowledge` to create a named collection and paste text. `/app/memory`
supports direct assertions, exact source evidence, correction/history and forgetting.
[Private knowledge](knowledge.md) explains search and the different deletion scopes.
[The knowledge agent](knowledge-agent.md) explains tool verification, run limits and
revocation of answers derived from withdrawn evidence.

Use `/settings/sources` for account-owned [data connections](data-connections.md):
opt-in Gmail sync, Slack user-token sync and custom HTTP producers. Configure the
operator's [Google OAuth app](mail.md) once in `/admin/google` (guided callback copy,
web-client JSON import and own-account check); each member then connects their own
Google account and selects collection scope. Slack currently uses a personal user
token, not a managed OAuth setup. The screen exposes pause/disconnect, safe sync
history, narrow ingest-token rotation and source provenance. The
[Rust sender example](../crates/moosuri-connectors/examples/custom_source_sender.rs)
can exercise the custom protocol using synthetic data.

## HTTPS reverse proxy

The process listens on loopback. Production needs a TLS reverse proxy and an
exact public HTTPS origin; `--dev` only permits explicit loopback HTTP origins.
Do not set the origin from an incoming proxy header.

```sh
moosuri serve --origin https://assistant.example.test \
  --trusted-proxy 127.0.0.1 --client-ip-header x-real-ip
```

The proxy must preserve the browser Host and **overwrite** `X-Real-IP` with its
observed client IP. Never append an untrusted incoming value. The server trusts
that header only from the configured TCP peer and accepts exactly one IP.
Without proxy configuration, forwarded headers are ignored. Login attempt
buckets also include normalized email digests, so one account's failures do not
lock every family member behind a proxy/NAT. KDF and writer admission are bounded.

A TLS-enabled Nginx location can forward to the loopback process as follows;
certificate/server-name configuration remains the installer's responsibility:

```nginx
location / {
    proxy_pass http://127.0.0.1:3210;
    proxy_set_header Host $http_host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_buffering off;
}
```

Session authorization has a server-enforced 30-day idle timeout renewed on
activity. The locator cookie has a 400-day browser cap and is HttpOnly,
SameSite=Strict and Secure outside explicit local development. Read and logout responses
do not overwrite cookies. Logout revokes the supplied session in SQLite; the
remaining unusable locator is replaced at the next login, so a delayed logout
response cannot erase another tab's newer login. Private requests assert their expected account ID;
this is compared with session authority, never used to create authority.
Password changes revoke previous sessions. A narrow journal-before-DB crash
window locks the affected account. A local operator can enroll a fresh temporary
credential using the stopped-installation command below. This is not an encrypted
backup/restore workflow or a production-ready release.

`moosuri doctor` checks a **stopped** installation. A running installation holds
its writer lock and correctly rejects another writer. Normal shutdown drains the
writer before releasing the lock. Stopped encrypted [exports](backup.md) are available after independent recipient
setup. Historical restore, upgrades and legacy migration remain unavailable;
copying a live DB is not a supported backup.

## Recovering a journal-locked account

Stop the installation, use its existing account UUID, and explicitly choose the
current role. Supply a new password of 12–256 UTF-8 bytes, different from the revoked password, through an owner-only
regular file belonging to the current OS user (one hard link, no symlink). No
password value belongs in command arguments or logs.

```sh
moosuri --data-dir /absolute/private/data recover-account \
  --account-id ACCOUNT_UUID --role member \
  --password-file /absolute/private/new-password
```

Use `--role admin` only when explicitly enrolling an administrator. The command
only accepts an existing account marked `recovery_required` by auth-journal
replay; it cannot reset a healthy account or initialize a missing installation.
It issues no session. Log in using the supplied temporary password within
24 hours, then change it before using private APIs. Previous sessions and old
execution/connector authority remain revoked. Other accounts are unchanged.

An interruption after the new journal record but before the SQLite commit locks
the account again on restart; repeat enrollment with a new credential. If commit
succeeded but the response was lost, the supplied temporary credential remains
valid and repeating recovery is rejected with `account_not_recovery_required`.
Do not clear journal files or flip the recovery flag manually. The separate sealed-current
[restore](restore.md) path supports fresh enrollment; historical restore and independently
maintained off-host latest journals remain unavailable.

## Local instruction-package authoring

These commands work before `--data-dir` handling and need no installation or server:

```sh
./target/debug/moosuri skills init synthetic-summary
# Edit the four files and review their job, examples and redistribution rights.
./target/debug/moosuri skills check synthetic-summary
# To create inside a repository, provide a new destination with an existing parent:
./target/debug/moosuri skills init synthetic-summary --output community/skills/synthetic-summary
```

Ordinary repository paths are allowed. Existing destinations, `..`, symlink components and installation-data paths are rejected. `check` accepts exactly the documented four bounded regular nonexecutable files and reports compatibility, not safety or task-quality certification. It performs no install, model call, Git operation or publishing. Follow your normal edit/review/commit/PR process; existing curated pins are unchanged. See [community authoring](../community/README.md#local-authoring).

## Local verification

CI/Actions are disabled at the owner's request. Run locally before committing a
coherent unit, then push the verified unit. No live model or production secret is
needed for these checks:

```sh
npm ci --ignore-scripts --prefix web
npm run typecheck --prefix web
npm test --prefix web
npm run build --prefix web
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked -p moosuri -- export-types | cmp - web/shared/api-types.ts
cargo run --locked -p xtask -- verify-plan
npm ci --ignore-scripts --prefix tools/openspec
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec validate --all --strict --no-interactive
```

Keep one reusable Cargo target directory per toolchain/target instead of copying
build output into each review checkout. For smaller development builds, set
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_INCREMENTAL=0`;
leave debug information enabled when investigating a crash. Source snapshots
should exclude Cargo's declared target directory, installed `node_modules` and
package download caches. Do not recursively delete folders by name: dependencies
can contain real source directories named `target`. Serialize writers to a shared
build directory. After its processes have stopped, disposable
build output can be removed with `cargo clean --target-dir /absolute/build/target`.
Preserve pending source edits, validation reports and installation data separately.

Build React **before** the Rust binary/installation integration test so the public
shell is embedded. Changing React requires a rebuild of the binary. For hot
reload, run Rust with `--origin http://127.0.0.1:5173 --dev` and use `npm run dev --prefix web`; Vite preserves Host while proxying API requests to port3210.

Generated DTOs come from `moosuri-core`. Regenerate `web/shared/api-types.ts` with
`moosuri export-types` after changing the contract. Product behavior is tested
with synthetic file-backed SQLite, real loopback HTTP and React fixtures.
Manual Chromium viewport checks do not establish real iOS/Android behavior or a
Linux release. Current measured evidence and remaining work are in
[status](status.md).

The optional [ChatGPT evaluation](reviews/2026-11-13-live-chatgpt-quality.md) is
explicitly ignored by ordinary tests. It requires separately authorized existing
Codex login and writes only a private report outside the checkout.

## Isolated sealed-checkpoint development

`backup checkpoint --seal-installation --output ABSOLUTE.age --head-output ABSOLUTE.json`
permanently retires only the explicitly selected stopped installation. Use dedicated
synthetic roots for development; a matching [restore](restore.md) is available,
while the source remains permanently sealed. `backup
checkpoint-status` reports only local stored phase/opaque IDs, not transfer or restore
readiness. The [checkpoint contract and limits](checkpoints.md) distinguish six-entry
sealed snapshots from ordinary export formats. The integrated artifact16/schedule17/account18/
checkpoint19/restore20/media21/conversational-reminder22 path passed independent review and Linux checks, including
late-publication admission errors; see [evidence](reviews/2026-11-13-sealed-restore.md).

Matching sealed cutover [restore](restore.md) verifies and quarantines a private destination before runtime admission.

## Optional separate vault

The [vault service/client guide](vault-service-client.md) documents actual Rust/WASM/React build order, separate UID/origin/cookie setup and exact incomplete release boundaries. Build `vault-web` before its Rust binary and avoid concurrent dist rebuilds during cargo/rustdoc. Assistant schema0024 is reserved for root integration; this standalone increment uses schema21.

### Personal ChatGPT native helper

The existing `moosuri` executable contains both the browser-computer helper and Linux SSH-stdin receiver. Build the normal locked native application with `cargo build --locked -p moosuri`; place a matching locally built binary in the Linux user's normal SSH command path using the project's existing deployment process. This work adds no automatic installer or signed download channel.

On the browser computer run `moosuri subscriptions chatgpt-vm --server https://your-server.example`. Missing email and existing SSH host alias are prompted; the password is hidden. Both initial and recovery login request a password-authenticated transfer-only session with an immutable ten-minute deadline, before any preparation. It cannot change passwords, create account/vault/source authority or run chat. Normal TLS validation and existing SSH host-key trust are required. Reconnect adds `--binding ID --revision N`; a distinct add-account omits both. If a transfer acknowledgement is lost, rerun the same helper before starting another login. An unfinished fresh-login attempt uses `--restart` and its retained issued registration. Never copy another tool's credentials or put tokens/cookies/keys in arguments.

The remote fixed command is `moosuri subscriptions chatgpt-receive --stdin`. It reads one bounded frame from the authenticated SSH channel and calls the running server's owner API. It does not open the installation DB, need an operator account or own a sealing key. After confirmed transfer, return to personal model settings for catalog/text proof and selection. Choose **도구 연결 확인** for that saved model/effort to verify foreground tools independently. A successful text check does not enable tools; an already admitted text run keeps its frozen capability. Exact owner/binding revision/model/effort proof is stored in schema36 and removed by sealed-restore quarantine. See the [actual member/helper/API composition](reviews/2026-11-14-personal-chatgpt-tools.md). Real owner OAuth/SSH/live plan availability remains separate acceptance from synthetic tests. GitHub Actions/automatic CI stays disabled.

Tool-verified personal ChatGPT also uses the existing explicit memory and record
planners: remember/correct/forget one current fact, or create/edit a local record.
Each planner resolves current personal access before its model request. Confirmations
come from actual Store receipts; a prepared memory retry can finish without another
model call. Record-writing stages use saved evidence only and omit live external read
definitions. See [local-write acceptance](reviews/2026-11-14-personal-chatgpt-writes.md).
Tool-verified personal chat can also create, edit within the same date, or delete an
explicit timed solo event on the connected primary Calendar. Missing Calendar access
links to `/settings/calendar`; connect there and send a new request. A disconnected
personal model retires unfinished Calendar work while retaining completed history.
See [Calendar acceptance](reviews/2026-11-14-personal-chatgpt-calendar.md). Background
delegation, recurring/all-day/attendee/secondary-calendar writes and media remain separate.

### Optional public community catalog

The serve flags `--community-catalog-repository`, `--community-catalog-ref` and `--community-catalog-directory` select one already published static GitHub artifact directory. All three are required; no source is enabled by default. See [community catalog runtime](community-skills.md) for publication prerequisites and the example. Cards access opportunistically checks the public index daily with ETag; manual refresh is rate bounded. Only chosen artifacts are downloaded, and the existing exact-commit GitHub adapter independently verifies original package bytes before staging. Public cache schema35 preserves schema34 and user lifecycle state. This unit does not publish artifacts, run an unattended scheduler or automatically activate work.

### Experimental credential-broker foundation

The workspace contains `moosuri-credential-broker`, a native enforcement library.
The default assistant has no broker HTTP routes or task credential adapter. For
protocol development only, `cargo build --locked -p moosuri --features moosuri-server/credential-broker-foundation` enables the experimental server routes;
there is no companion executable or production invocation-minting path. Synthetic
probe authority exists only in `test-fixtures`. See the [foundation guide](credential-broker-foundation.md)
and its [verification record](reviews/2026-11-14-credential-broker-foundation.md).
