# Security reporting

Do not post credentials, private conversations, vault files, exploit secrets or production dumps in public issues or PRs.

Use GitHub private vulnerability reporting at [Security → Report a vulnerability](https://github.com/codemoo/moosuri/security/advisories/new). The maintainer enables this channel when creating the public repository. There is no supported production release yet. If it is unavailable, use a maintainer's explicitly published private contact before sharing sensitive details; do not invent an email address or place those details in a public issue.

A useful report describes the affected version, trust boundary, minimal synthetic reproduction and observable impact. A reproducible example with fake users/credentials is preferred. The current project is a planning package, not a deployed Rust assistant.

Vault keys and credentials remain outside LLM context. Community skills do not grant new authority or execute arbitrary scripts. The concrete threat boundaries and their limits are in [vault security](docs/vault-security.md), [credential use](docs/credential-use.md), [user isolation](docs/accounts-isolation.md) and [community skills](docs/community-skills.md).
