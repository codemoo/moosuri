# Community recipes

Contributors submit skills to this repository. Users can inspect and add the six curated instruction recipes through **제안 / 할 수 있는 일** at `/app/skills`. Installation is local to the authenticated account and does not execute a task. Until first managed installation, these recipes remain bundled baseline guidance. Installation replaces the matching bundled procedure with a single managed pin. Disable/remove persist a tombstone that suppresses the procedure in new runs; underlying native tools remain available. Reinstall restores the managed procedure. This is an instruction catalog, not an executable package registry.

The runtime pins each bundled recipe's revision and content digest when accepting a run. The model sees a short description and reads the full procedure only when relevant. See [task skills](../docs/task-skills.md) for capabilities, limits and acceptance evidence. A manifest cannot install new tools or grant authority.

Use `skills/<slug>/SKILL.md`, a small `moosuri.toml`, synthetic `examples.json`, and a redistribution LICENSE. The first installer accepts exactly these four files, at most 32 KiB per file and 64 KiB total. Optional references/assets and external archives remain future work and are rejected by this installer. A future `moosuri skills init <slug>` command can generate the boilerplate; it is not implemented yet. Until then create the files using the sample shape in [the design](../docs/community-skills.md).

A good recipe has one clear job, says when to use it, names the existing capabilities it needs, and describes how to verify the result. Include a normal example and the most consequential failure/unsupported-input example. More tests are justified by new failure modes, not by copying the old test tree.

`SKILL.md` follows [Agent Skills](https://agentskills.io/specification). The moosuri sidecar is a narrow version-1 compatibility contract for this curated catalog, not a universal TOML/Agent Skills implementation. Only the documented single-line scalar/array syntax, current allowlisted capability names, instruction mode and Apache-2.0 are accepted. Plain instruction import is not implemented.

Never include live tokens, account IDs, personal traces, extracted private skill text or downloaded assets without rights. Do not add install scripts/hooks; v1 runs only existing typed moosuri capabilities. Adding a connector is a Rust contribution governed by OpenSpec, not a shell command embedded in a skill.

Open a normal PR with purpose, compatibility, license and examples. The source catalog is reviewed in Git; a trusted release job later builds the immutable catalog/package artifacts. Package downloads require no new community account. GitHub publishing is an explicit contributor action; private learning is never automatically uploaded.

See [CONTRIBUTING](../CONTRIBUTING.md), [distribution and user experience](../docs/community-skills.md), [credential use](../docs/credential-use.md) and [verification](../docs/testing.md).

The initial `catalog-pins.json` covers the exact four-file bytes from source commit `7f91c0ad8ef38049c5e0e601658da9b99b5f824e`. SHA-256 uses big-endian u64 byte-length framing for repository, commit, slug, then sorted filename/content pairs. The Rust catalog checks this manifest before exposing cards. Updating upstream files alone cannot silently activate a new package. A future trusted release generator must resolve a new committed source snapshot and produce a new manifest; no release job or automatic download is implemented here.

SQLite is authoritative for install/update/rollback/enable/disable/remove and hidden suggestions. Updates validate and stage immutable bytes in the same transaction as the active pointer; interruption retains the old pointer. Disable excludes the package from new runs. Remove also cancels and invalidates already admitted runs and blocks retries while retaining public package snapshots and completed receipts. Per-user activation and hiding never affect another account. Private-learning export, local package edits, dependent schedules, HTTP archive caches and lockfile portability remain unimplemented.

## Public GitHub acquisition

The UI now accepts a public GitHub repository root, package directory and ref through **GitHub 원본에서 추가**. Preview resolves one exact commit and validates four bounded regular instruction files; one install/update action activates those exact bytes for the current account. The fixed public API client has no Git process, archive extraction, authentication, cookies or private context. Git blob SHA-1 verifies object-byte consistency; app package identity remains SHA-256. Neither is safety certification.

External version2 envelopes/opaque IDs include canonical repository and directory. They do not replace bundled twins or rewrite existing curated hashes/pins. Native tool/connection authority remains separate. A manual update retains old run snapshots and uses current-user revision CAS. The current compatibility contract still supports only the documented four-file, instruction-mode, Apache-2.0 subset. Private GitHub, generic imports, scripts/assets, automatic refresh, export and GC remain future work.

For a read-only public adapter probe: `cargo run --locked -p moosuri-connectors --example inspect_public_skill -- https://github.com/codemoo/moosuri community/skills/source-briefing 7f91c0ad8ef38049c5e0e601658da9b99b5f824e`. This prints only public source identity/file counts and does not install anything.
