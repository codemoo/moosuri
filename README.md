# moosuri

A self-hosted assistant for individuals, families and friends.

**Early development preview. Start with [current support and remaining work](docs/status.md).**
Rust backend/agent/CLI + React/TypeScript web + SQLite. The preview includes isolated
accounts, durable chat with bounded tab-refresh draft recovery, private knowledge and memories, editable records, bounded
read-only missions from chat or forms, personal Gmail, bounded Calendar actions from chat or forms, browser workspaces, local reminders,
opt-in Web Push, private images/UTF-8 files/text PDFs, bounded voice dictation through a
separate OpenAI API connection, one-use PWA text/link sharing, and a separate encrypted vault.
Every mail send needs the user's explicit approval of its final preview. Unknown sends
have conservative read-only Sent checks and are never automatically resent.

Suggestions can discover an explicitly configured public GitHub skill catalog,
verify selected original packages and keep each user's installations separate.
See [community skills](docs/community-skills.md) for configuration and remaining limits.

This is the public development repository [codemoo/moosuri](https://github.com/codemoo/moosuri),
not a stable release or a migration of the existing private deployment. Its source
history, accounts, raw communications, credentials, operating data and infrastructure
are preserved separately. Public examples and evidence use synthetic data.
GitHub Actions/automatic CI remain disabled; contributors run relevant checks locally.

## Start

- [Development quickstart](docs/development.md): build, init, shared API/local model settings and run the embedded web app. LLM configuration can be skipped while setting up accounts.
- [Current status](docs/status.md): implemented bounds, evidence and next work. Optional vault/broker packages are still required v1 deliverables; the full product is not complete.
- [Requirements](docs/requirements.md) · [feature contracts](docs/features.md) · [rewrite plan](docs/plan.md) · [migration order](docs/migration.md).

## Use and configure

- [Accounts](docs/account-management.md) · [onboarding](docs/onboarding.md) · [isolation](docs/accounts-isolation.md) · [model settings](docs/provider-settings.md) · [provider support](docs/providers.md).
- [Chat](docs/chat.md) · [private knowledge](docs/knowledge.md) · [conversational memory](docs/conversational-memory.md) · [automatic extraction](docs/automatic-memory.md) · [documents, goals and followups](docs/artifacts.md).
- [Task skills](docs/task-skills.md) · [community packages](docs/community-skills.md) · [missions](docs/missions.md) · [reminders/inbox](docs/schedules.md) · [Web Push](docs/web-push.md).
- [Personal Gmail](docs/mail.md) · [personal Calendar](docs/calendar.md) · [source connections](docs/data-connections.md) · [public web research](docs/web-research.md) · [private browser workspaces](docs/browser-workspaces.md).
- [Images](docs/media.md) · [files and text PDFs](docs/private-files.md) · [PWA](docs/pwa.md) · [vault service/client](docs/vault-service-client.md) · [encrypted vault archive](docs/vault-archive.md) · [vault service recovery](docs/vault-service-recovery.md) · [credential broker contract](docs/credential-use.md).
- [Stopped encrypted export](docs/backup.md) · [sealed-current restore](docs/restore.md) · [release/installation contracts](docs/public-release.md). Ongoing historical/off-host recovery and general update/uninstall are still pending.

Account-owned Gmail/Slack collection and generic custom ingest expose scope, sync
receipts and provenance. The operator configures one Google OAuth app; each member
grants their own account access. Slack uses a personal user token. Synthetic acceptance
does not establish live-provider readiness. Kakao collection is private and excluded
from public code/packages; independent producers can use the custom protocol.

The model adapters support configured API/compatible/local endpoints with actual
capability probes. Personal ChatGPT supports same-computer official sign-in, sealed owner-scoped sessions,
text-probed personal model selection and saved streamed chat, plus protected native
transfer to a personal HTTPS Linux server. Separately verified personal foreground tools
support saved reads, exact calculation and the existing connected read/local-draft flows.
Personal memory/artifact/Calendar mutations, media and background work remain pending.
See [foreground-tool acceptance](docs/reviews/2026-11-14-personal-chatgpt-tools.md), including the inherited combined-tool limit. Synthetic protocol
and lifecycle checks do not establish live account eligibility. Real device/provider acceptance remains bounded by
[current status](docs/status.md); a model name or connection label does not prove support.

## Develop

[CONTRIBUTING](CONTRIBUTING.md) · [OpenSpec](openspec/README.md) · [architecture](docs/architecture.md) ·
[testing](docs/testing.md) · [source audit](docs/audit.md) · [public release](docs/public-release.md).
Personal coding-agent instructions are ignored; shared behavior and decisions belong
in the canonical documents. Validate planning metadata with Rust1.89 or newer:

```sh
cargo run --locked -p xtask -- verify-plan
```

Licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
