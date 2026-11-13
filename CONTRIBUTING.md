# Contributing to moosuri

Start with [README](README.md) and [current status](docs/status.md). The development preview includes bounded agent/model execution, a separate vault service/client and per-user community package management alongside the account/API/CLI and React foundation. Their full contracts and release gates remain incomplete; current status distinguishes implemented slices from pending integration and future work. See [development](docs/development.md) for the exact runnable boundary. Do not report proposed checks as passing product tests.

## A small contribution path

1. Pick one issue or explain the concrete behavior in the PR. An issue is useful for coordination, not mandatory paperwork for a small fix.
2. For new behavior, compatibility/security/storage changes or a new extension contract, create an OpenSpec change. Typos, styling and small fixes preserving a contract can use a direct PR.
3. Make the smallest coherent change with a useful failure-focused test. Do not copy legacy tests or reproduce implementation details in assertions.
4. Run the checks relevant to the change and record actual results/limitations. Open a draft PR early if another contributor owns the same files.

Ordinary community recipes using existing capabilities need a small skill PR and synthetic examples, not a full design ceremony. A new connector, credential scope or installation format needs an OpenSpec change. Human/AI contributors use the same process; an authorized implementation request does not require another permission question at every stage.

## OpenSpec

Use the repository-pinned OpenSpec **2.0.0** from `tools/openspec`; do not silently upgrade a contributor's global CLI. Node is a contributor tool only. The product backend remains Rust and the web apps React.

```sh
npm ci --ignore-scripts --prefix tools/openspec
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec new change <change-id>
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec instructions proposal --change <change-id> --json
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec status --change <change-id> --json
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec validate --all --strict --no-interactive
```

`proposal.md` explains the problem/scope, delta `specs/` gives observable requirements and scenarios, `design.md` explains material tradeoffs, and `tasks.md` records implementation and verification. Keep all four artifacts concise when using the default schema. `status` checks artifacts, not working software. Archive only completed changes after their spec/code/tests are consistent; never archive as a CI side effect.

Current upstream editor skills use `openspec-*` names (Codex uses `$openspec-propose`, etc.). The propose workflow is planning-only. When implementation is already requested, use the ordinary task plus direct CLI artifacts/apply workflow; do not add repeated approval stops. Optional editor setup is a contributor choice. We do not commit personal generated assistant sessions or modify global skills/settings.

Baseline [specs](openspec/README.md) contain established repository contracts. Unimplemented product work remains in `openspec/changes/`. `docs/requirements.md` and `features.md` are indexes; `architecture.md` explains design; `status.md` records what actually works. For a capability governed by OpenSpec, update its delta spec and link the docs rather than copying a second competing requirement list. The newest user-approved scope remains controlling.

## Parallel work and review

Use a branch or worktree per change. Mark owned files in the draft PR and discuss overlapping API/schema edits before both sides implement them. Keep one coherent behavioral change per PR; a storage migration and its Rust/React contract can stay together when atomic review helps. Do not rewrite unrelated files, force-reset someone else's work, or regenerate locks incidentally.

Review should focus on observable outcomes, migration/rollback, failure paths and public API compatibility. Auth, external effects, vault, credential broker, package installation and restore boundaries need an independent reviewer. Routine changes use normal review; no extra approval service or mandatory committee is introduced. The initial maintainer is [@codemoo](https://github.com/codemoo). Work is committed and pushed in coherent, locally verified units. Branch protection must not require disabled checks.

## Checks and useful tests

Currently runnable:

```sh
cargo fmt --all --check
npm ci --ignore-scripts --prefix web
npm run typecheck --prefix web
npm test --prefix web
npm run build --prefix web
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked -p moosuri -- export-types | cmp - web/shared/api-types.ts
cargo run --locked -p xtask -- verify-plan
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec validate --all --strict --no-interactive
```

Follow [testing](docs/testing.md). State the defect a test would catch, keep the oracle independent, and consolidate duplicate coverage. Existing test counts/source names are evidence, not a porting backlog. Build React before the Rust installation test so its public assets are embedded. Commands for later capabilities become required when their implementations exist. A green syntax/plan check is not a product-quality claim.

GitHub Actions and automatic CI are disabled during initial development at the owner's request. Run the commands above locally and include their results with each work unit. The inactive workflow template is in `tools/ci/check.yml.disabled`; do not enable it or add a live workflow without a later explicit request. Local coding-agent instructions (`AGENTS.md`, `CLAUDE.md` and personal editor directories) are ignored and are not contribution prerequisites. Public OpenSpec and community skill files stay versioned.

If CI is re-enabled later, fork PR CI must use read-only permissions and synthetic data with no model/provider/production/vault keys. Never use `pull_request_target` to execute untrusted contribution code. Paid evaluations and release publishing are separate trusted jobs with explicit budgets/scopes. Do not upload raw conversation/provider traces as CI artifacts.

## Community skills, rights and reporting

See [community authoring](community/README.md) and [the card/distribution design](docs/community-skills.md). Contributors publish to Git; users discover through the assistant's suggestions and capabilities. No registry account is needed for public downloads. Share original generic recipes with synthetic examples and an explicit redistribution license. Never publish a user's learned trace, credentials or private instructions.

New code and documentation are licensed under MIT OR Apache-2.0, at your option; see [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE). Contributions are provided under the same terms unless explicitly agreed otherwise. A package's license must also be compatible with its included assets/references. The source audit is reference metadata, not a license grant for the private legacy source.

Use issue templates for ordinary bugs/features and [SECURITY](SECURITY.md) for vulnerabilities. Report version, steps, expected/actual outcome and sanitized diagnostics, never passwords, tokens, real messages, vault exports or private model reasoning.
