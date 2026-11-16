# Shared execution tool policy — 2026-11-16

Scope: static tool exposure/admission for live guidance, missions and task monitors;
synthetic source only. No production service, legacy data, provider credential or
cron was used. GitHub Actions remain disabled.

## Findings and changes

- Mission model exposure used a negative name filter while Store used a positive
  typed list. `parcel_history`, Gitea and Google terminal tools could be advertised
  from broad profiles despite being unsupported by mission execution. Exact shared
  capability predicates now omit those tools before inference.
- Live instruction eligibility was enforced by runner/profile setup without an
  explicit durable Store tool-class fence. Admitted live channels now reject
  artifact/mail/mission effects, browser and terminal native requests before slots
  are committed. This remains in force before guidance, after application and at
  the receipt cap. Store channel setup also keeps existing foreground profile
  exclusions; it cannot widen rights by bypassing runner setup.
- Mission admission and tool execution failures returned silently, leaving the
  current run active. The driver now uses the existing Store failure mapping and
  finish transition, including partial status for exhausted original tool budgets.

`moosuri-core/src/tool_policy.rs` is the closed built-in capability contract.
Runner tool names and Store typed inputs use it. This is an upper bound rather
than authority: current profile pins, account/epoch checks, exact Calendar/mail
selection, recipe route/argument checks, report/effect contracts, source membership,
byte limits, cumulative budgets, leases and publication fences still apply.
Task monitors retain their exact source/revision and range guards. A read-only
recipe remains usable; compound draft/report routes still need their existing
original contracts and never inherit effects from their static classification.

## Evidence

- Direct stable `rustfmt --edition 2024 --config skip_children=true` on changed Rust
  modules and `git diff --check`: passed.
- New synthetic regressions cover forbidden live-channel effects before guidance,
  after applied guidance and after the eight-receipt cap while pinned skill reads,
  exact calculation and argument repair continue; broad mission profiles omit
  unsupported parcel/native/browser tools; a run-wide overbudget mission round
  settles partial without restart or another model call.
- With the isolated Rust1.89 native environment, these locked/offline filters passed:
  `moosuri-core tool_policy` (2); `moosuri-store live_read_policy` (1),
  `live_instruction` (5), `task_monitor` (4); `moosuri-agent mission_definitions`
  (1), `run_instructions` (1), `connected_missions` (5),
  `mission_rejected_tool_round` (1), and `tests::missions` (15). Filters overlap;
  these counts are not an aggregate quality score. The mission filters include
  fresh form execution, continuation/reopen, original-budget repair, personal
  model scope/revocation and compound report behavior.
- Targeted `cargo clippy --locked --offline --no-deps -p moosuri-core -p
  moosuri-store -p moosuri-agent --all-targets -- -D warnings`: passed.
  Dependency-inclusive strict clippy stopped in unchanged credential-broker code:
  `local_browser.rs:673` suspicious-else formatting and `policy.rs:121`
  too-many-arguments. Those unrelated modules were not changed or suppressed.
- Independent effect-boundary review found the resumed tool-error branch still
  returned silently. Fresh/replay execution now share one settlement helper; a
  reopened committed-slot budget failure settles partial with no new inference.
  The follow-up review found no remaining material boundary issue. Integrated
  public evaluator workflows passed; final platform evidence is in the
  [integrated review](2026-11-16-everyday-assistant.md).

No new tools, permission prompts, schemas, native/vault rights, mail send authority,
Calendar effects or report grants were added. Live account/device acceptance and
broad semantic quality are outside this synthetic refactor.

## Independent review follow-up: restart replay

Review found that the committed-slot replay branch still silently returned on a
Store execution error while fresh execution settled the mission. Both paths now
share `settle_tool_result`, preserving the same failure mapping and finish guard.
A dev-only dependency on the already locked SQLite library supports an actual
stopped synthetic DB injection; there is no runtime fixture hook or new authority.

The regression commits a pending arithmetic slot, stops Store, injects an exhausted
local tool budget and reopens. Replay settles partial with no verified result and
only the original single model dispatch. Locked/offline agent filters passed:
`mission_resumed_committed_local_error` (1), `mission_rejected_tool_round` (1) and
`mission_first_tool_round` (2 successful replay cases). Targeted agent all-target
`clippy --no-deps -- -D warnings` and `git diff --check` also passed. These results
cover this follow-up; they do not broaden the earlier dependency-inclusive lint or
live-provider acceptance claims.
