# Claude subscription preparation and private login bridge

This is progress on M05/M06, not completed product setup or natural-language
Gmail acceptance. Production remains the accepted dee84754 bundle. No provider
credentials were copied, no model configuration was replaced, and no inference
was made by these checks. The existing model-selection question remains open.

## Actual Linux prerequisites

In the marked disposable Ubuntu 24 installation guest, a separate test identity
ran rootless Docker 29.1.3 under a systemd user service with cgroup2. Only that
identity received controller delegation. The existing member Kiri services were
not used as model workers. This manual fixture is evidence for implementing
automatic preparation; it is not that public preparation implementation.

The unchanged official Claude Code 2.1.322 Linux artifact's manifest signature
and SHA256 were verified. Under the existing profile's container flags, its
version command ran with memory 805306368, CPU quota/period 100000/100000, PIDs 32,
NoNewPrivs 1 and Seccomp 2. Host source, authentication and engine sockets were
unavailable inside; direct host/metadata/public egress failed and there was no
external route. A credential-free existing worker correctly reported
`claude_worker_account_unavailable`, removed its container and left no worker
socket. The experimental profile is not a production isolation attestation.

References: [Docker rootless requirements](https://docs.docker.com/engine/security/rootless/),
[rootless service tips](https://docs.docker.com/engine/security/rootless/tips/),
[official Claude setup](https://code.claude.com/docs/en/setup).

## Changes and verification scope

`Profile::verify` now requires rootless Docker, systemd/cgroup 2 and the reported
memory, CPU-period/quota and PIDs capabilities. The old rootless-name check alone
could accept an engine that silently ignored resource limits. This remains a
reported capability check; the actual confinement checks above are separate.
The 42 connector tests passed serially on Linux. Earlier parallel fixture timing
failures were retained, not described as passing. Independent official Claude
Sonnet 5.5 / medium review found no material defect in the resource-check scope.

The new private child command reuses protected worker configuration, profile,
exact email and `Profile::login_web`. It has a bounded JSONL channel for one
validated official authorization URL, one code or cancellation, and a fixed
terminal status. It performs initialization catalog checks only, with no user
inference or runtime binding write. Worker and child share owner/revision/model
validation and verify all configured model/effort choices.

The parent must provide private pipes. Reactor-backed input/output replaces
Tokio's blocking stdin reader, which could otherwise keep a successful process
alive while the parent left its input open. The protocol handles cancellation,
EOF, partial input, invalid/duplicate/out-of-order codes, deadline and output
loss. A cancelled operation is driven through its existing container cleanup
before reporting the result. Raw authentication output is never an error string.

Final Linux checks passed: 15 engine-login tests, 42 connector tests, and 12
actual-binary process cases, including successful exit with the parent's input
still open, partial EOF, duplicate code, invalid configuration/profile, additional
unavailable model, regular-file output rejection and cancellation while the exact
environment lock is held. The existing private-email fixture initially depended
on the invoking umask; it now explicitly sets 0700 without relaxing product
checks. Two unrelated ChatGPT fixtures failed with the default umask; the final
full engine suite passed with 077. These are synthetic processes, not real account
or provider eligibility tests.

A separate real Linux check ran the candidate helper and the unchanged official
Claude binary in the guest's existing rootless confinement. The private child
emitted an official `https://claude.com/cai/oauth/authorize` URL; the harness
kept its parameters in memory, cancelled without opening a browser or submitting
a code, and observed `cancelled`, process exit and removal of the owned container.
The parent's input pipe remained open until exit. No worker socket, inference
or runtime binding was created. This took 34.59 seconds including catalog and
startup. The experimental profile was not promoted to production attestation.

Independent official Claude Sonnet 5.5 / medium review and correction review found
no material defect in the final bridge scope. Review prompted cancellable
preflight cleanup, zeroizing command deserialization and an explicit existing
serde feature dependency. The parent inspected those changes, the fixture and
the process harness. Linux build and targeted Clippy completed; Clippy retained
pre-existing warnings outside the changed paths, so no warning-free workspace
claim is made.

Reproduce the synthetic checks on Linux with a private umask:

```sh
umask 077
cargo test --locked -p moosuri --features test-fixtures --bin moosuri engine_login -- --test-threads=1
cargo test --locked -p moosuri-connectors claude_code -- --test-threads=1
cargo build --locked -p moosuri --bin moosuri
python3 deployment/kiri/test_claude_login_bridge.py --binary target/debug/moosuri
```

## Remaining product integration

The gateway must own a child only for the originating authenticated session,
owner generation and administrator model revision. It must never forward URLs
or codes into conversations, logs or persistent browser storage. Before writing
a binding it must recheck authority, start the existing worker and verify the
actual Unix peer and readiness. The child itself grants no installation authority.

Automatic artifact/service/profile preparation, bounded persistent auth storage,
Kiri login presentation, binding publication, revocation and restart recovery
remain open. The eventual public flow must generate local units and configuration;
the manual operator guide does not satisfy an empty-server user journey. A second
member needs a separate authenticated environment and binding.

The [eight supporting journeys](../../openspec/changes/automate-installation-google-app/journeys.md#m05m06-subscription-connection-checkpoints--8-supporting-journeys)
retain these limits alongside the 61 primary acceptance journeys. C01 remains
open until real natural-language Gmail works through an admitted model connection.

Follow-up implementation and native preparation journeys are recorded in
[the runtime preparation report](2026-11-27-claude-runtime-preparation.md).
