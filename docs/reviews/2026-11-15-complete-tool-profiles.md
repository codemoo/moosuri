# Complete foreground tool composition regression

## Defect and admitted bounds

At2157adc the combined fixture covered28 definitions, while Store admission also
enables conversation recall2 and every bundled skill (public-page collection1).
One usable owner recipe adds the learned executor1. The complete default foreground
composition therefore contains32 definitions. With `credential-broker-chat`, Store
adds four terminal credential and four native Gmail definitions, reaching40.
Native-mail scope is admitted even when its binding-choice list is empty; actual
use still checks the selected owner's grant. Shared and personal foreground chat
use this same admission path. This is an attainable configuration, not an arbitrary
sum of mutually exclusive write/read stages.

Evidence: `moosuri-store/src/web.rs::admitted_profile`,
`agent_execution/skills.rs::profile`, `native_task_mail.rs::admit_profile`,
`learned_skills.rs::admit`, and the production tool-definition builders. Multiple
connections or recipes expand pins/enum values, not function names.

The complete40-definition fixture first failed with `LimitExceeded` under the old
32-definition constant, after its exact composition-count assertion passed. The
new48-definition ceiling preserves the64KiB aggregate schema/description ceiling,
16KiB schema bound, depth/closed-schema/name validation and unknown-call rejection.
Per-round calls, cumulative dispatch/tool budgets, original deadlines, actual
capability probes, owner pins, model/effort and final mail-send approval are unchanged.
No definition is dropped to fit the ceiling.

## Observed checks

Native offline checks in an isolated checkout based on2157adc:

- `cargo test --locked --offline -p moosuri-agent --features credential-broker-chat combined_tools_tests -- --nocapture`:3 passed with all40 definitions.
- The same agent filter without the feature:3 passed with all32 definitions.
- `cargo test --locked --offline -p moosuri-connectors model::protocol::tools::tests -- --nocapture`:15 passed, including48 accepted/49 rejected across all shared encoders and the personal namespace, aggregate64KiB denial and unknown-call rejection.
- Strict `cargo clippy --locked --offline --no-deps -p moosuri-agent -p moosuri-connectors --features moosuri-agent/credential-broker-chat --all-targets -- -D warnings`:passed. Formatting and `git diff --check`:passed.

The initial unrestricted agent Clippy command stopped on two existing broker lints:
`local_browser.rs:673` (`suspicious_else_formatting`) and `policy.rs:121`
(`too_many_arguments`). Those unrelated files were not changed. The targeted
strict checks above cover the changed crates and test targets.

Every supported encoder accepts the complete production-builder composition with
default text capabilities. Both real loopback HTTP fixtures inspect every exact
name, strict schema and account pin on the initial and continuation requests, then
complete a decimal call/result/final exchange. These fixtures use synthetic
credentials and explicit synthetic tool output. They establish composition and
transport behavior, not full Store admission, external-adapter execution, actual
provider eligibility, live latency improvement or release acceptance. No live
model, production data, legacy service, DB or cron was accessed.
