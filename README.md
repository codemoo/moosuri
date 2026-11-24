# moosuri

Moosuri is a self-hosted personal assistant for individuals, families and friends. Members have separate conversations, memories and connected accounts. Only the installation administrator chooses the model and reasoning effort for chat and internal inference. [Model settings](docs/provider-settings.md)

Moosuri is in development preview. The public installer runs the standard server. The Linux deployment uses a Kiri container per member and a shared inference engine. Features and verification differ between the two. [Current support and remaining work / 현재 상태](docs/status.md)

## Start

Run this command in a macOS or Linux terminal, or inside a Windows WSL2 distribution. Choose English or 한국어 and follow the prompts to create the first administrator account.

```sh
curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash
```

For installer requirements, updates and troubleshooting, see the [installation guide / 설치 안내](docs/install.md). To operate the per-member runtime on Linux, follow the [Kiri operator guide](deployment/kiri/README.md).

## Use and configure

| I want to… | Where to start |
| --- | --- |
| Open the web app from my devices | [Tailscale](docs/tailscale.md) · [Public HTTPS](docs/public-access.md) · [PWA](docs/pwa.md) |
| Manage members, the engine or Google connections on Kiri | [Administration](deployment/kiri/admin.md) · [Engine connections](deployment/kiri/subscriptions.md) · [Google Workspace](deployment/kiri/google.md) |
| Review task timelines or schedule a search in my Kiri workspace | [Member tasks](docs/member-tasks.md) |
| Follow a browser task in my Kiri session | [Member browser](deployment/kiri/browser.md) |
| Explore standard-server features | [Chat](docs/chat.md) · [Memory](docs/conversational-memory.md) · [Missions](docs/missions.md) · [Skills](docs/community-skills.md) · [Connections](docs/data-connections.md) · [Vault](docs/vault-security.md) |

## Develop

Moosuri uses Rust for the server, agent and CLI, React/TypeScript for the web clients, and SQLite for storage. Start with the [development quickstart](docs/development.md) and [contribution guide](CONTRIBUTING.md). Automatic CI is disabled. Run the checks relevant to your change locally.

For shared design decisions, start with the architecture. For a Kiri member feature, also read the member runtime design. Current support and deployment are recorded in the status page. Dated reviews preserve the evidence for the version they tested. [Architecture](docs/architecture.md) · [Kiri member runtime](openspec/changes/isolate-account-runtime/design.md) · [Current status](docs/status.md)

The requirements and feature contracts define product scope. OpenSpec tracks individual changes and the testing guide links their acceptance evidence. [Requirements](docs/requirements.md) · [Feature contracts](docs/features.md) · [OpenSpec](openspec/README.md) · [Testing](docs/testing.md)

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
