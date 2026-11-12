# Community recipes

Contributors submit skills to this repository; end users will eventually find installable skills in the assistant's **제안 / 할 수 있는 일** cards. The installer/catalog runtime and these cards are still planned. Three original bundled recipes currently run through chat: `source-briefing`, `meeting-preparation` and `evidence-comparison`. They use existing typed knowledge reads and exact decimal arithmetic; this folder is not an executable package registry.

The runtime pins each bundled recipe's revision and content digest when accepting a run. The model sees a short description and reads the full procedure only when relevant. See [task skills](../docs/task-skills.md) for capabilities, limits and acceptance evidence. A manifest cannot install new tools or grant authority.

Use `skills/<slug>/SKILL.md`, a small `moosuri.toml`, synthetic `examples.json`, and a redistribution LICENSE. Optional references/assets must be bounded and properly licensed. A future `moosuri skills init <slug>` command can generate the boilerplate; it is not implemented yet. Until then create the files using the sample shape in [the design](../docs/community-skills.md).

A good recipe has one clear job, says when to use it, names the existing capabilities it needs, and describes how to verify the result. Include a normal example and the most consequential failure/unsupported-input example. More tests are justified by new failure modes, not by copying the old test tree.

`SKILL.md` follows [Agent Skills](https://agentskills.io/specification). The moosuri sidecar is a proposed native compatibility contract and must not be presented as a finalized universal standard. A minimal instruction skill without machine-readable requirements can be imported for inspection, but is not automatically advertised as ready to run.

Never include live tokens, account IDs, personal traces, extracted private skill text or downloaded assets without rights. Do not add install scripts/hooks; v1 runs only existing typed moosuri capabilities. Adding a connector is a Rust contribution governed by OpenSpec, not a shell command embedded in a skill.

Open a normal PR with purpose, compatibility, license and examples. The source catalog is reviewed in Git; a trusted release job later builds the immutable catalog/package artifacts. Package downloads require no new community account. GitHub publishing is an explicit contributor action; private learning is never automatically uploaded.

See [CONTRIBUTING](../CONTRIBUTING.md), [distribution and user experience](../docs/community-skills.md), [credential use](../docs/credential-use.md) and [verification](../docs/testing.md).
