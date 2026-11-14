# Personal official Claude Code text

This optional integration executes Anthropic's unchanged official Linux CLI. It
connects owner settings, an exact-model text probe, personal model selection and
saved Runtime conversations. The base Rust application needs no Claude CLI,
Node/Python SDK, container engine or subscription. Claude.ai bearer tokens and
session cookies are never imported into the Rust HTTP model adapter.

The implemented acceptance tests use synthetic child processes and accounts.
Actual Linux rootless-container isolation, the pinned official artifact, official
login and real account/model eligibility remain separate deployment acceptance.
A configured profile is not evidence that these live checks have passed. The
available acceptance host currently has rootful Docker and no rootless Podman;
this profile intentionally rejects it. Do not change host services to bypass this.

## Supported boundary

Only text is enabled. Tools, attachments, live model catalog, observed effort,
background/mission execution, memory writes, Calendar/mail operations and remote
CLI session resume are unavailable. Supply an exact `claude-...` version ID;
aliases are not advertised as an account catalog. The official account still has
to accept that exact model in the owner's actual probe. No effort echo is exposed
by the CLI stream to applications, so this integration offers unspecified effort.

Each inference has a fresh official session and process with session persistence
disabled. Canonical Moosuri history is sent as labelled, quoted conversation data
alongside the current request, rather than claiming native CLI transcript replay.
The trusted system prompt is a private file. Input uses the official stream-JSON
`client_composed:true` field, preventing slash command execution and @file/MCP
expansion. Content never enters argv, environment variables or diagnostics.

The existing `begin_agent_step` transaction commits the invocation/usage receipt
before process launch or stdin. Output remains partial until the decoder sees
consistent model/message/session/input identity, a valid terminal result, clean
EOF and exit 0. Truncation, malformed suffix, cancellation, timeout and lost result
stay unknown after dispatch; no provider inference is automatically repeated.
Restart reconciles labelled stale containers using full IDs and local durable
state. It never invokes global `--continue` or `--resume latest`.

After historical restore, text probes reserve one new inference allowance in the
SQLite dispatch transaction before launching the process. The transaction rechecks
the owner, current binding revision and exact environment/profile/root. Invalid
local input spends nothing; failed/unknown attempts consume their slot. Saved chat
uses the ordinary step admission, and the last admitted call can still finish.
Historical accounting remains unknown. See [recovery allowance](recovery-forward-budget.md).

Reported query-pipeline cumulative usage is retained per model, including auxiliary
models, and its token totals are aggregated once for the run. Missing counters stay
unknown. A valid result followed by a failed process exit retains observed usage
without claiming completion. CLI estimates and pipeline counters are not a billing
statement, complete helper-call accounting or remaining subscription allowance.

## Operator profile

Use an operator-installed **unchanged official Claude Code 2.1.322 Linux binary**
and the matching native Linux Moosuri executable as the container helper. Pin
both executable SHA256 values, the Docker executable and an existing OCI image
by immutable image digest. Do not add proprietary binaries or private profiles
to the source repository, Git, Dropbox, public image artifacts or logs. Official
CLI distribution and licensing remain separate from Moosuri's source license.

The current profile supports rootless Docker through an explicitly owned Unix
socket. It does not accept rootful Docker or substitute an unsandboxed process.
The API cannot supply an executable, image, mount, endpoint or filesystem path.
The engine receives an explicit empty Docker config and no inherited environment.
Every launch rechecks pinned files and rootless engine status, disables pulling,
uses no external network namespace interfaces, drops capabilities, requires
no-new-privileges and constrains processes/CPU/memory. Host repositories, other
users' homes, keychain/D-Bus/SSH-agent sockets and the engine socket are not mounted
inside the container. The image must provide only the trusted CLI/helper runtime
and required libraries/certificates; it must contain no credentials or projects.

Create existing private 0700 auth/scratch roots and store a 0600 operator profile
outside the checkout. Values below are placeholders, not a working installation:

```json
{
  "engine": "/usr/bin/docker",
  "engine_sha256": "<64 hex characters>",
  "engine_socket": "/run/user/1000/docker.sock",
  "image": "registry.example/runtime@sha256:<64 hex characters>",
  "official_cli": "/opt/private-runtime/claude",
  "cli_sha256": "<64 hex characters>",
  "cli_version": "2.1.322",
  "helper": "/opt/moosuri/moosuri",
  "helper_sha256": "<64 hex characters>",
  "auth_root": "/srv/moosuri-private/claude/auth",
  "scratch_root": "/srv/moosuri-private/claude/scratch",
  "isolation_acceptance_sha256": "<SHA256 of credential-free deployment acceptance>",
  "hosting_terms_accepted": true
}
```

`isolation_acceptance_sha256` records an operator's deployment attestation. It is
not a self-verifying substitute for the tests below. Preserve the exact image,
helper, official binary, kernel/engine version, source/protocol version and
acceptance record together. Profile changes invalidate use of previously pinned
bindings; reconnect with a new owned environment after acceptance. Pending cleanup
is scoped to the auth-root identity, so executable/profile updates on the same
root can still finish old retirement work. Drain retirement ACKs before moving or
removing the auth root. Local retired-environment tombstones must be retained.

Start with `moosuri serve ... --claude-profile-file /private/path/profile.json`.
The installation must independently verify that the image supports these exact
binaries, filesystem quotas and policy constraints. Safe/restricted flags are
additional controls, not an OS sandbox. Safe mode preserves authentication but
managed policy, including managed hooks, can still apply. Do not bypass an
organization's policy. An incompatible or unverifiable managed-policy account
must remain unavailable for this profile. Empty local settings and init metadata
alone do not prove that no startup hook executed.

The container has `--network=none`. Its internal Rust helper offers only a local
CONNECT proxy bridged through a per-invocation private Unix socket. The host proxy
accepts only port 443 and a fixed official-origin allowlist, rejects nonpublic DNS
answers and pins the chosen public address for that connection. There is no
custom inference URL, ambient proxy, model-selected destination or TLS bypass.
The helper has a persistent parent lease; losing the host bridge ends the helper
and its container. Each bridge/output/request/connection lifetime is bounded.

## Owner login and selection

In **My model → My Claude**, add an owned binding. On the same Linux server,
run the displayed native helper with the exact operator profile:

```sh
moosuri subscriptions claude-login --server https://assistant.example.com \
  --email my-address --binding <owned-id> --revision 1 \
  --profile-file /private/path/profile.json
```

The helper asks privately for the Moosuri password, validates
the owned binding/profile and runs the official `claude auth login` inside that
binding's environment. Authentication choices remain those of the unchanged CLI;
each user uses their own official account and billing agreement. No Claude token
is sent through the Moosuri API. A remote browser-only login is not implemented;
use the official CLI's own supported terminal/browser completion flow on that
server. The exact login UX still requires deployment acceptance.

Login has a bounded lifetime and polls the authenticated current binding/session;
a revoked/expired Moosuri session or changed binding cancels it. Server and native
helpers use an OS file lock per environment so concurrent refresh/login/inference
cannot share mutable auth state. Each environment has independent HOME,
CLAUDE_CONFIG_DIR, XDG roots, private cwd/temp and processes. Nothing reads an
ambient local Claude login or copies a Mac keychain/credential file.

After official login, return to the UI and explicitly run the exact text-model
probe; this real probe uses the user's account. A successful probe enables that
exact binding/revision/profile/model choice, with tools false and effort null.
Select it in the ordinary personal model selector, then use saved chat. A synthetic
probe fixture is never shown as successful real-account acceptance.

Disconnect blocks future dispatch immediately and fences active output. An opaque
durable retirement queue then tombstones and removes the owned host auth directory
without reading its contents, and records a cleanup ACK. Account deletion keeps
`local_complete=false` until this credential cleanup ACK exists. Missing runtime
configuration or a filesystem failure leaves cleanup pending; it is not reported
as complete. Tombstones prevent an old native launch reference from recreating a
retired directory. Removing local credentials is not proof of remote revocation;
use the official account settings to revoke remote sessions as needed. Sealed
restores disable old bindings and enqueue their credential retirement; no old
selection/login is revived.

## Credential-free acceptance

Run the focused Rust `claude` tests for connectors, Store and agent, plus
`ClaudeExecutionSettings.test.tsx` and existing provider-settings tests. They use a
synthetic engine subprocess, fake stdout events, synthetic private-file canaries
and dedicated databases. They verify actual child argument/environment boundaries,
byte-pin failures, clean-EOF/exit rules, main/aux accounting, retirement/tombstones,
owner lookup, personal selection, saved Runtime output, cancellation/revoke and
unknown-without-replay. The fixture does not run Docker or Claude and therefore
cannot prove Linux namespace isolation.

Before enabling a deployment, separately run the same production launch profile
with a synthetic Linux child and canaries. Verify sibling-owner/host mounts and
symlink escapes, host sockets/proc/FDs, direct DNS/LAN/metadata/internet egress,
parent loss and descendant cleanup, auth filesystem byte/inode quotas, resource
ceilings, managed-policy injection and exact immutable artifact/profile identity.
Then validate the pinned unchanged official binary's no-inference startup/profile
behavior. User-performed official login and real model probe remain explicit later
acceptance. Keep receipts/counts/digests and fixed statuses, not raw auth or model
traces, in the acceptance record.

Primary evidence checked 2026-11-15: official
[legal/compliance](https://code.claude.com/docs/en/legal-and-compliance.md),
[authentication](https://code.claude.com/docs/en/authentication.md),
[CLI reference](https://code.claude.com/docs/en/cli-reference.md),
[headless](https://code.claude.com/docs/en/headless.md), and
[published message types](https://code.claude.com/docs/en/agent-sdk/typescript.md).
Hosting/preinstalling unchanged Claude Code requires the applicable Commercial
Terms and documented per-end-user authentication/billing conditions. The SDK/token
relay restriction is not permission to implement a consumer OAuth HTTP adapter.
