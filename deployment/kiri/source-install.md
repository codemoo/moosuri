# Clean Linux source installation

This development path prepares the member Kiri runtime from a selected public
Git commit. It is under acceptance testing; it does not yet replace the standard
one-line installer or complete automatic Google OAuth application issuance.
See [current support](../../docs/status.md) and the
[remaining Google setup work](../../openspec/changes/automate-installation-google-app/tasks.md).

The starting machine is Ubuntu 24.04 amd64 with systemd, root access, Git and
outbound HTTPS. Kiri, Docker, Node, Rust, GWS, application data and Google Cloud
resources do not need to be prepared. Source compilation needs at least 4 GiB
RAM and 30 GiB available disk; native compilation and downloads take time.
Two different DNS names must point to this server, with ports 80 and 443
reachable for the automatic certificate path.

From a public clone:

```sh
git clone https://github.com/codemoo/moosuri.git
cd moosuri
sudo bash deployment/kiri/install-from-source.sh \
  --public-origin https://assistant.example.com \
  --vault-origin https://vault.example.com \
  --certbot-email operator@example.com --accept-certbot-terms
```

The installer prompts for the first administrator's name, email and hidden
password. Keep the chosen password: a retry uses it to authenticate through the
ordinary member login and password routes. It is not written into installation
receipts, service arguments or build artifacts. The default capacity is four
members with a 10 GiB ceiling per member; `--member-capacity` and
`--member-volume-gib` select other limits before installation.

The script selects one commit and excludes uncommitted files. Rust tooling
prepares pinned Kiri 0.2.2 and builds the host binary, embedded assistant/Vault
web clients, Vault WASM and separate member, browser and official Chrome images.
GWS and Google API discovery files are built into the member image. Base image
digests and downloaded Chrome/GWS assets are pinned. Ubuntu/Debian package
repositories remain mutable: this is not a bit-for-bit reproducible or signed
release bundle. The manifest checks integrity of the artifacts selected by root.

The Rust `setup-kiri` command checks every bundle artifact before changing
product state. It refuses foreign files or a conflicting existing installation.
It uses the existing registry, capped member storage, invitation and lifecycle
paths. Browser configurations are admitted only after actual empty-profile
Chromium/Chrome startup, kernel confinement and owned-container cleanup pass.
The separate-origin Vault and Google recipient transport enroll members through
their authenticated gateway sessions; no fixed owner's recipient or private
developer helper is required.

The printed recovery directory contains the completed bundle. After an
interruption, run that bundle's `moosuri setup-kiri --bundle …` with the same
origins, member limits and administrator identity. Installation and first-member
request IDs stay the same. Changed inputs are refused; do not delete private
state to force a retry. Root-owned supplied certificates are supported with the
paired `--tls-certificate` and `--tls-key` options instead of Certbot.
If a previous failed attempt exhausted member login attempts and reports
`member_login_rate_limited`, wait five minutes before resuming the same command.
Do not replace the password or delete the member to work around the limiter.

This unit prepares installation, browser, Vault and GWS transport. It does not
claim a connected Google account: new app creation, actual consent and Gmail
read must pass the separate end-to-end gate. The candidate installs `moosuri-kiri-model.service` before member onboarding.
Without a provider it serves explicit unconfigured member sockets. An admin
can register or replace the installation Anthropic API key and select a model
from the management screen; keys and config live in root-only
`/var/lib/moosuri-control/model-setup`, outside the member registry. A saved
configuration is not proof of working inference: verify a conversation. This
API path does not create Claude subscription workers. A malformed provider file
at service startup is rejected until corrected; runtime key/config recovery is
retried without admitting invalid calls. Final first-provider acceptance and
the single default public installer entry remain open. No Mac credentials or existing Cloud project may satisfy those gates.
