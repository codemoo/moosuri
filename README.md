# moosuri

A self-hosted assistant for individuals, families and friends.

**Early development preview. Start with [current support and remaining work](docs/status.md).**
Rust backend/agent/CLI + React/TypeScript web + SQLite. The preview includes isolated
accounts, durable chat with bounded tab-refresh draft recovery and older-conversation recall,
private knowledge with recorded-time source filters and opt-in fixed Korean word aliases, and memories with bounded date-scoped conversational updates, editable records, bounded
read-only missions from chat or forms with editable paused one-off criteria and preserved request history, explicitly delegated recurring Gmail/Calendar briefings, saved followup date notifications, Japan Post latest-summary lookups, personal Gmail with bounded PDF/text attachment reads, provider drafts and approved fixed scheduled sends, bounded timed/all-day Calendar actions and follow-up clarification from chat or forms, browser workspaces with optional login-profile recovery within their original lifetime, local reminders with bounded missing-time/content replies,
opt-in Web Push, per-schedule notification mute, private images/UTF-8 files/text PDFs, bounded voice dictation through a
separate OpenAI API connection, one-use PWA text/photo/file/audio sharing, and a separate encrypted vault.
The experimental [native credential companion](docs/native-credential-github.md)
can unlock the independent live vault or an explicit encrypted archive locally and
read one exact GitHub repository with a selected credential. Live mode rechecks the
current selected item before use and resumes after transient connection loss. An explicit exact-repository chat request can wait for the
same signed result and finish in its original conversation without another model call.
The [Google companion adapter](docs/native-credential-google.md) can also read the
selected account's Gmail INBOX total/unread counts. A separately selected
[INBOX preview](docs/native-google-inbox-preview.md) shows up to five literal
subject/from/date/snippet summaries in settings or the original chat. OAuth refresh
and account/scope verification stay on the device. Preview mail remains outside
later model history, conversation recall and learned recipes.
Its routes remain default-off; general credential tools and signed companion
distribution remain unfinished.
An explicit [legacy v1 encrypted-file import](docs/vault-v1-migration.md) locally
authenticates the old export before re-encrypting selected items for the current vault.
Account deletion clears active private data and delivers durable deletion to the separate vault.
Every mail send needs the user's explicit approval of its final preview, including scheduled timing. Unknown sends
have conservative read-only Sent checks and are never automatically resent.

Private learned skills can reuse verified two-to-four-step exact calculations and Japan Post latest-summary lookups with fresh inputs. A [source-bound calculation](docs/evidence-calculation.md) also composes one unambiguous saved-source read with exact arithmetic, then re-reads the current source on reuse. These routes can export a structural recipe with new synthetic examples.
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
- [Linux ARM64 development](docs/linux-arm64.md): cross-build and direct emulation evidence; native hardware and official release acceptance remain open.
- [Signed HTTPS download](docs/signed-download.md): fetch an explicit versioned bundle with an independent signing pin; verify before any install.
- [Local signed installation](docs/local-installation.md): install an explicitly trusted native bundle, update a stopped installation from supported schema47 through the current version, recover interrupted activation and remove runtime files while retaining data. Official release bundles are not yet published.
- [Current status](docs/status.md): implemented bounds, evidence and next work. Optional vault/broker packages are still required v1 deliverables; the full product is not complete.
- [Requirements](docs/requirements.md) · [feature contracts](docs/features.md) · [rewrite plan](docs/plan.md) · [migration order](docs/migration.md).

## Use and configure

- [Accounts](docs/account-management.md) · [onboarding](docs/onboarding.md) · [isolation](docs/accounts-isolation.md) · [model settings](docs/provider-settings.md) · [provider support](docs/providers.md) · [server response timings](docs/run-timings.md).
- [Chat](docs/chat.md) · [private knowledge](docs/knowledge.md) · [conversational memory](docs/conversational-memory.md) · [automatic extraction](docs/automatic-memory.md) · [documents, goals and followups](docs/artifacts.md).
- [Task skills](docs/task-skills.md) · [private learned routes](docs/learned-skills.md) · [community packages](docs/community-skills.md) · [missions](docs/missions.md) · [reminders/inbox](docs/schedules.md) · [Web Push](docs/web-push.md).
- [Personal Gmail](docs/mail.md) · [personal Calendar](docs/calendar.md) · [source connections](docs/data-connections.md) · [normalized call transcripts](docs/call-feed.md) · [selected local work sessions](docs/local-work-sessions.md) · [public web research](docs/web-research.md) · [Japan Post tracking](docs/parcel-tracking.md) · [private browser workspaces](docs/browser-workspaces.md).
- [Images](docs/media.md) · [files and text PDFs](docs/private-files.md) · [PWA](docs/pwa.md) · [vault service/client](docs/vault-service-client.md) · [encrypted vault archive](docs/vault-archive.md) · [vault service recovery](docs/vault-service-recovery.md) · [routine vault snapshots](docs/vault-ongoing-backups.md) · [pinned later-journal recovery](docs/vault-historical-recovery.md) · [periodic vault journal receiver](docs/vault-journal-publication.md) · [credential broker contract](docs/credential-use.md).
- [Private transfer import](docs/legacy-import.md) creates a new account from an explicit normalized source/memory/document bundle with reconciliation and quarantine reports. The [schema23 relation adapter](docs/legacy-relations.md) converts an explicitly exported isolated PostgreSQL snapshot; real operator export and production cutover remain pending.
- [Live full-data snapshots](docs/online-backups.md) · [stopped encrypted export](docs/backup.md) · [historical restore](docs/historical-restore.md) · [periodic journal receiver](docs/live-journal-publication.md) · [release/installation contracts](docs/public-release.md). Periodic publication retains a recovery-point gap; actual off-host deployment, zero-loss latestness and official release acceptance remain pending.

Account-owned Gmail/Slack collection and generic custom ingest expose scope, sync
receipts and provenance. An explicit once-only CLI can import selected human Codex
requests from a chosen local directory. A separate explicit call importer accepts
normalized transcripts and optionally transcribes up to20-minute canonical WAV files
in bounded chunks, with settled-file hashes and per-owner ingestion receipts.
Unknown/mixed call speakers are excluded from automatic personal-memory extraction.
Vendor phone exports and other work-session formats remain unsupported. The operator configures one Google OAuth app; each member
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
explicit primary-calendar timed/all-day solo-event creation, date edits and deletion
through the existing typed service. Explicit one-off and recurring read-only missions
can use the owner's separately verified personal model after logout, including
delegated Gmail/primary Calendar reads. Media, automatic memory extraction and
mission writes/browser/vault execution remain pending.
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
The evaluator also includes a [local paired baseline adapter](docs/agent-evaluation.md#두-공통-세계의-로컬-실행-adapter) for two deterministic common outcomes. Its actual legacy comparison remains unrun because the local host lacks free System V shared-memory slots; this is not a model-quality result.
[Dependency inventory](docs/dependency-inventory.md) provides an offline Rust generator for a CycloneDX source-lock SBOM.
[Rust notice evidence](docs/rust-license-evidence.md) and [web notice evidence](docs/web-license-evidence.md)
collect exact attribution from reviewed local sources and identify missing evidence.
Optional [notice supplements](docs/notice-supplements.md) attach exact upstream
text with separate source claims and coverage while preserving local evidence gaps.
[Release provenance](docs/binary-provenance.md) binds executable bytes and that inventory
to a signed manifest with declared source/compiler metadata. The installer verifies
the binding before mutation; this does not authenticate the build process itself.
Personal coding-agent instructions are ignored; shared behavior and decisions belong
in the canonical documents. Validate planning metadata with Rust1.89 or newer:

```sh
cargo run --locked -p xtask -- verify-plan
```

Licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
