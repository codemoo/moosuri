# moosuri

A self-hosted assistant for individuals, families and friends.

**Early development preview. Start with [current support and remaining work](docs/status.md).**
Rust backend/agent/CLI + React/TypeScript web + SQLite. The preview includes isolated
accounts, durable chat with bounded tab-refresh draft recovery and older-conversation recall,
private knowledge and memories, editable records, bounded
read-only missions from chat or forms, explicitly delegated recurring Gmail/Calendar briefings, saved followup date notifications, Japan Post latest-summary lookups, personal Gmail with provider drafts and approved fixed scheduled sends, bounded Calendar actions and follow-up clarification from chat or forms, browser workspaces, local reminders,
opt-in Web Push, per-schedule notification mute, private images/UTF-8 files/text PDFs, bounded voice dictation through a
separate OpenAI API connection, one-use PWA text/photo/file/audio sharing, and a separate encrypted vault.
The experimental [native credential companion](docs/native-credential-github.md)
can unlock the independent live vault or an explicit encrypted archive locally and
read one exact GitHub repository with a selected credential. Live mode rechecks the
current selected item before use and resumes after transient connection loss. An explicit exact-repository chat request can wait for the
same signed result and finish in its original conversation without another model call.
Its routes remain default-off; general credential tools and signed companion
distribution remain unfinished.
Account deletion clears active private data and delivers durable deletion to the separate vault.
Every mail send needs the user's explicit approval of its final preview, including scheduled timing. Unknown sends
have conservative read-only Sent checks and are never automatically resent.

Private learned skills can reuse verified two-to-four-step exact calculations with fresh inputs and export a structural recipe with new synthetic examples.
Suggestions can discover an explicitly configured public GitHub skill catalog,
verify selected original packages and keep each user's installations separate.
Disabling a used recipe pauses its dependent recurring tasks until the original
version is available and the owner explicitly resumes; unrelated schedules continue.
See [community skills](docs/community-skills.md) for configuration and remaining limits.

This is the public development repository [codemoo/moosuri](https://github.com/codemoo/moosuri),
not a stable release or a migration of the existing private deployment. Its source
history, accounts, raw communications, credentials, operating data and infrastructure
are preserved separately. Public examples and evidence use synthetic data.
GitHub Actions/automatic CI remain disabled; contributors run relevant checks locally.

## Start

- [Development quickstart](docs/development.md): build, init, shared API/local model settings and run the embedded web app. LLM configuration can be skipped while setting up accounts.
- [Local signed installation](docs/local-installation.md): install an explicitly trusted native bundle, update a stopped installation from supported schema47 through the current version, recover interrupted activation and remove runtime files while retaining data. Official release bundles are not yet published.
- [Current status](docs/status.md): implemented bounds, evidence and next work. Optional vault/broker packages are still required v1 deliverables; the full product is not complete.
- [Requirements](docs/requirements.md) · [feature contracts](docs/features.md) · [rewrite plan](docs/plan.md) · [migration order](docs/migration.md).

## Use and configure

- [Accounts](docs/account-management.md) · [onboarding](docs/onboarding.md) · [isolation](docs/accounts-isolation.md) · [model settings](docs/provider-settings.md) · [provider support](docs/providers.md) · [server response timings](docs/run-timings.md).
- [Chat](docs/chat.md) · [private knowledge](docs/knowledge.md) · [conversational memory](docs/conversational-memory.md) · [automatic extraction](docs/automatic-memory.md) · [documents, goals and followups](docs/artifacts.md).
- [Task skills](docs/task-skills.md) · [private learned routes](docs/learned-skills.md) · [community packages](docs/community-skills.md) · [missions](docs/missions.md) · [reminders/inbox](docs/schedules.md) · [Web Push](docs/web-push.md).
- [Personal Gmail](docs/mail.md) · [personal Calendar](docs/calendar.md) · [source connections](docs/data-connections.md) · [selected local work sessions](docs/local-work-sessions.md) · [public web research](docs/web-research.md) · [Japan Post tracking](docs/parcel-tracking.md) · [private browser workspaces](docs/browser-workspaces.md).
- [Images](docs/media.md) · [files and text PDFs](docs/private-files.md) · [PWA](docs/pwa.md) · [vault service/client](docs/vault-service-client.md) · [encrypted vault archive](docs/vault-archive.md) · [vault service recovery](docs/vault-service-recovery.md) · [credential broker contract](docs/credential-use.md).
- [Private transfer import](docs/legacy-import.md) creates a new account from an explicit normalized source/memory/document bundle with reconciliation and quarantine reports. An actual legacy schema/export adapter and production cutover remain pending.
- [Stopped encrypted export](docs/backup.md) · [historical restore](docs/historical-restore.md) · [periodic journal receiver](docs/live-journal-publication.md) · [release/installation contracts](docs/public-release.md). Periodic publication retains a recovery-point gap; actual off-host deployment, live full-data backup and official release acceptance remain pending.

Account-owned Gmail/Slack collection and generic custom ingest expose scope, sync
receipts and provenance. An explicit once-only CLI can import selected human Codex
requests from a chosen local directory; other client formats remain unsupported. The operator configures one Google OAuth app; each member
grants their own account access. Slack uses a personal user token. Synthetic acceptance
does not establish live-provider readiness. Kakao collection is private and excluded
from public code/packages; independent producers can use the custom protocol.

The model adapters support configured API/compatible/local endpoints with actual
capability probes. Personal ChatGPT supports same-computer official sign-in, sealed owner-scoped sessions,
text-probed personal model selection and saved streamed chat, plus protected native
transfer to a personal HTTPS Linux server. Separately verified personal foreground tools
support saved reads, exact calculation and the existing connected read/local-draft flows.
Explicit current-user memory changes and source-grounded record creation/anchored edits
use typed local operations and stored confirmations. Personal Calendar also supports
explicit primary-calendar timed solo-event creation, same-day edits and deletion
through the existing typed service. Media and background work remain pending.
An optional [official Claude Code text connection](docs/claude-execution.md) provides
owner settings, an exact-model probe and saved chat through a separate pinned
rootless Linux execution profile. Actual official login and production-profile
isolation acceptance remain pending; tools/media/background are unavailable.
See [foreground-tool acceptance](docs/reviews/2026-11-14-personal-chatgpt-tools.md),
[personal Calendar](docs/reviews/2026-11-14-personal-chatgpt-calendar.md),
and the [combined-tool limit repair](docs/reviews/2026-11-14-combined-tool-profiles.md). Synthetic protocol
and lifecycle checks do not establish live account eligibility. Real device/provider acceptance remains bounded by
[current status](docs/status.md); a model name or connection label does not prove support.

## Develop

[CONTRIBUTING](CONTRIBUTING.md) · [OpenSpec](openspec/README.md) · [architecture](docs/architecture.md) ·
[testing](docs/testing.md) · [synthetic outcome runner](docs/agent-evaluation.md) · [source audit](docs/audit.md) · [public release](docs/public-release.md).
[Dependency inventory](docs/dependency-inventory.md) provides an offline Rust generator for a CycloneDX source-lock SBOM.
Personal coding-agent instructions are ignored; shared behavior and decisions belong
in the canonical documents. Validate planning metadata with Rust1.89 or newer:

```sh
cargo run --locked -p xtask -- verify-plan
```

Licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
