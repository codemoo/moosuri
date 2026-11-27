# moosuri

Moosuri is a self-hosted personal assistant for individuals, families and friends. Members have separate conversations, memories and connected accounts. Only the installation administrator chooses the model and reasoning effort for chat and internal inference. [Model settings](docs/provider-settings.md)

Moosuri is in development preview. The Linux server uses a Kiri container per member, a separate encrypted Vault and a shared inference engine. The local standard-server preview has a different installation and feature set. [Current support and remaining work / 현재 상태](docs/status.md)

## Start

For a **Linux server**, start on an empty Ubuntu 24.04 amd64 system. Prepare two
DNS names for the assistant and Vault, pointing to the server, with ports80/443
reachable. In an interactive terminal:

```sh
git clone https://github.com/codemoo/moosuri.git
cd moosuri
sudo bash deployment/kiri/install-from-source.sh
```

The source build takes time. Setup then asks for the two HTTPS addresses,
certificate contact/terms, and first administrator account. It installs Kiri,
the app and separate Vault without developer credentials or preinstalled model
tools. After sign-in, use **Management** for the model and member invitations,
then **Connections** for your Vault and Google account. Google/model acceptance
is still in progress. [Linux requirements, scripted setup and recovery](deployment/kiri/source-install.md)

For a **local standard-server preview on macOS, Linux or Windows WSL2**, use:

```sh
curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash
```

This signed preview is separate from the Linux member Kiri installation above.
[Installation and platform support / 설치 안내](docs/install.md) ·
[Kiri operations](deployment/kiri/README.md)

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
