# Native mail tool routing repair

2026-11-16. Scoped synthetic defect repair from source `9d72356ab0081c4440964e4d25284e76cfaeafbc`.

## Defect and resulting behavior

The shared name classifier omitted `native_mail_search`, `native_mail_message`,
`native_mail_thread` and `native_mail_attachment`. The mission schema filter
therefore removed these tools even from an admitted operation8 native-mail
profile. Typed Store admission already classified the same input as
`NativeMailRead`, which mission policy permits. Foreground native schemas were
unfiltered, so the failure appeared only when the same supported read was
explicitly delegated.

All four names now map to the existing `NativeMailRead` capability. This repairs
initial and resumed mission discovery; default-off builds, current model proof,
operation8 profile, exact account/device/grant/background and selector-lineage
execution checks remain authoritative. Live-instruction and task-monitor policy
still exclude native mail.

## Reproduction and verification

The existing real Runtime fixture was aligned with the current first-round
execution contract by removing its obsolete model-only planning response. The
user's native-mail summary request was unchanged. Before the mapping repair,
foreground signed wait/continuation succeeded, then delegated mission inference
failed its missing `native_mail_search` schema assertion and timed out waiting.
The bounded baseline failed 1/1; raw synthetic diagnostics remain outside Git.

After the repair, the same fixture checks the complete four-tool catalog at every
initial and resumed model request. It verifies the actual signed synthetic native
receipt, original chat run identity, no model polling during either wait,
delegated mission continuation, actual returned summary and four total inference
calls across the foreground and durable mission flows. A separate catalog test
checks classification against actual emitted schemas and retains missing flag,
missing/invalid scope, feature-off and restricted-policy denial.

Executed locally with Rust 1.89:

| Check | Result |
| --- | --- |
| `cargo test --locked -p moosuri-agent --features credential-broker-chat native_mail` | 2 passed |
| `cargo test --locked -p moosuri-core tool_policy` | 2 passed |
| `cargo test --locked -p moosuri-agent native_catalog_retains_mission_reads_without_enabling_instruction_surfaces` | 1 passed; feature off |
| `cargo fmt --all --check` and `git diff --check` | passed |

The repository-pinned OpenSpec CLI dependencies are absent in this isolated
clone; CLI spec validation was not run here. The [delta](../../openspec/changes/native-tool-routing/specs/native-tool-routing/spec.md)
records the repaired contract. These checks use a loopback model and synthetic
signed broker data. They do not establish real Google account/device acceptance,
broader assistant quality or release readiness. No production data, credentials,
CI configuration or model settings were changed.
