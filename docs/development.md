# Run the development preview

This is a development preview, not the finished assistant. It supports real
local accounts, session persistence, durable text conversations and an
administrator user list/invitation flow. Shared provider setup, model discovery,
synthetic verification and personal model/effort selection are available. Streaming,
cancel/retry, saved history and observed usage are connected. Private text sources,
lexical search and explicit memory editing are available. Tool-verified models can
search/read that private knowledge in chat. Automatic extraction, schedules, media,
vault, external actions and backup/restore
remain pending.

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
window locks the affected account for future operator recovery; that recovery
command is not implemented and this is not a production-ready release.

`moosuri doctor` checks a **stopped** installation. A running installation holds
its writer lock and correctly rejects another writer. Normal shutdown drains the
writer before releasing the lock. Backup/restore, upgrades and legacy migration
remain unavailable; copying a live DB is not a supported backup.

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

Build React **before** the Rust binary/installation integration test so the public
shell is embedded. Changing React requires a rebuild of the binary. For hot
reload, run Rust with `--origin http://127.0.0.1:5173 --dev` and use `npm run dev --prefix web`; Vite preserves Host while proxying API requests to port3210.

Generated DTOs come from `moosuri-core`. Regenerate `web/shared/api-types.ts` with
`moosuri export-types` after changing the contract. Product behavior is tested
with synthetic file-backed SQLite, real loopback HTTP and React fixtures.
Manual Chromium viewport checks do not establish real iOS/Android behavior or a
Linux release. Current measured evidence and remaining work are in
[status](status.md).
