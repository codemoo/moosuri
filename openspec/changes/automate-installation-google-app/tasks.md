# Work log

- [x] Reconcile scope with a clean public installation and no pre-existing Google resources.
- [ ] Package installation/runtime/browser/vault/GWS dependencies without developer files or fixed owner routing.
- [ ] Observe official Console steps in fresh Kiri and identify provider prerequisites.
- [ ] Implement resumable administrator setup and direct client registration.
- [ ] Integrate same-browser member consent and automatic GWS readiness.
- [ ] Validate Linux scenarios, obtain independent Claude review, deploy and publish.
- [ ] Complete real OAuth registration, member consent and one GWS Gmail read.

Implementation order within the public-installation gate (do not treat an
individual prerequisite as completion of Google setup):

1. Member-aware vault enrollment and lifecycle — published; production keeps
   single-member routing pending the portable Google transport.
2. Public browser launcher on verified Kiri — implementation and Linux browser
   acceptance and independent review complete; published as the browser prerequisite.
3. Member provisioning/lifecycle compatibility with the verified Kiri version,
   preserving existing allocation identities.
4. Public installation packaging and member/attempt-bound fresh official Chrome
   and credential recipient; remove private Python imports and owner socket.
5. Fresh unbilled Google project, OAuth branding/client and shared installation
   configuration, then same-browser per-member consent/GWS readiness.
6. Blank-server and two-member end-to-end acceptance, production rollout and
   actual Gmail read. Provider-required verification remains a visible pause.

Acceptance scenarios (run results must replace pending, not imply success):

| Feature | Scenarios | Result |
| --- | --- | --- |
| Public installation | clean Linux/public source; no developer home; no Kiri/GWS preinstallation; generated HTTPS/vault configuration; installation interrupted/retried; second installation has distinct identities | pending |
| Vault and member routing | empty vault creates account; one account reused; multiple accounts selected; second member enrolls automatically; recipient generation changes; another member cannot reach the first browser/key | pending |
| Administrator setup | no Cloud resources; no billing account; optional billing remains unlinked; provider requires billing but none exists; first-use terms; lost create response; resume partial setup; revoked admin; unexpected Console page | pending |
| Shared application | second member receives same app; private tokens remain separate; new member joins after setup; conflicting client preserved; test-audience restriction displayed | pending |
| Member connection | existing vault account; multiple-account choice; device verification; same-browser consent; identity mismatch; GWS distribution delayed; real Gmail read | pending |

Starting production state: selected vault Google account signed in successfully
on 2026-11-26; installation app, member grant and real API read remain incomplete.

## 2026-11-26 development checkpoint — not deployed

The user clarified that acceptance must start with a **new project**, and that
the observed operator flow must become product automation. No existing project
was changed and no OAuth client was created during this investigation. A fresh
Kiri session authenticated successfully and reached official Cloud Console.
The observed new-project form refused submission without a billing account.
Three active choices were shown, including two with identical display names.
This was one existing account's form, not evidence of a universal requirement.
The later clean-install correction supersedes the earlier billing-choice wait;
no choice is pending as a prerequisite for independent implementation work.

Implemented draft pieces: registry migration 8 and durable intentions/leases;
administrator rechecks and private Console transport; origin/project guards;
one Console tab at a time; member UI billing choice by ID, polling, cancellation
and response-loss reconciliation. These pieces are **not an end-to-end flow**.
`google_bootstrap::workflow` still stops after prerequisite inspection and `prepare`
is not called by the production connection path. Do not deploy this draft.

Linux checks: checkpoint scenarios 8 passed after review fixes; gateway Google
regression and selector checks 29 passed; TypeScript checks and 38 handoff /
Workspace UI tests passed, including 4 billing-choice cases. The previous helper
23 and Console adapter 7 passed, including the project-identity guard.
Counts describe component
checks, not successful OAuth issuance or real GWS access.

Independent Claude Sonnet 5.5 / medium reviewed the checkpoint component. The
duplicate-worker and billing-before-intent findings were corrected. Final
client-file provenance and crash adoption must be checked in the still-missing
registration integration, using the existing atomic/idempotent client writer.

The exploratory browser later reported `chrome_unavailable`; its container was
still running and did not report OOMKilled. No root cause is asserted. The
product adapter closes the previous Console tab before another page opens.
The exploratory session was discarded successfully; retry starts with fresh Kiri login.

## Clean-install correction — 2026-11-26

The controlling baseline is another person downloading the project with nothing
prepared. Inspection found four product gaps: the public installer targets the
standard server; Google connection uses `owner.sock`; its service depends on an
unpublished VM launcher and private browser modules; the vault adapter enrolls
one fixed account. Do not report the owner-specific flow as portable support.

Proceed by usable units, without turning each small test into a new task:

1. Package the existing Rust Kiri installation, vault enrollment and member-aware
   browser transport. Exit: clean Linux install and two independently routed
   members with no developer files.
2. Prepare a fresh installation Google app in the same isolated browser. Exit:
   no-billing baseline, explicit provider prerequisites, durable project/client
   reconciliation and direct secret registration.
3. Distribute the app, complete member consent and read Google data. Exit: real
   Gmail read by the initiating member and separate consent/token ownership for
   another member.
4. Independent security review, matched registry-consumer deployment and public
   release. Exit: public installation exercises the same accepted flow.

Component checks and existing-owner experiments cannot close these exit gates.

The draft checkpoint now permits a confirmed unbilled project, distinguishes a
provider-required billing account from no available account, and preserves that
decision across an uncertain create/restart. Linux checkpoint scenarios: 10
passed; gateway Google regressions: 29 passed. Independent Claude Sonnet 5.5 /
medium review found reusable worker identifiers, repeatable create intent and
noncanonical callback acceptance. These were corrected with persisted used-run
IDs, a new-intent compare-and-set, idempotent confirmation and exact canonical
callbacks; the 10 checkpoint scenarios passed afterward. These are development
component checks; the registry migration and protocol draft are not deployed.

The Linux source-only smoke used Python isolated mode with only
`deployment/kiri` added to its module path. `google_connection`,
`enroll_google_connection` and `google_console` each failed on missing
`fresh_browser`. This is a failed clean-install check, not a test environment
workaround to hide by adding the developer's private module directory.

### Real no-billing provider experiment

A new isolated Kiri Chrome signed in after Google device verification. Its
Console project form offered only existing billing accounts, so no account was
selected. In that same disposable Kiri container, the official Cloud SDK 594.0.1
was installed and authorized from an empty SDK configuration. The documented
unversioned Linux archive matched SHA-256
`c6fdba4cb1551b0da7c1d744eb0dfaf68b59861238d6a6b5ef6bad1ba596b567` and its embedded
version was checked. The version-named archive had different bytes and was not
executed under that checksum; packaging must pin the tested artifact exactly.

The official CLI created a fresh project and enabled Gmail, Drive, Docs, Sheets
and Slides APIs. Provider queries confirmed ACTIVE, all five APIs enabled,
`billingEnabled=false`, and no billing account linked. No existing project or
client was reused. The new project's OAuth overview displayed its initial
configuration form. No branding or OAuth client was saved in this experiment.

This proves that the observed Console billing requirement is avoidable through
a supported unbilled creation route. It does not simulate an account that has
never accepted Google's terms, and it does not prove a clean public Moosuri
installation, automatic OAuth issuance, member consent or a real Gmail read.
The temporary operator harness must become the existing Rust-owned setup path;
it is not a product implementation or another required user-operated tool.

### Portable member vault unit — 2026-11-26

- [x] Add installation-wide enrollment while retaining explicit single-member mode.
- [x] Inherit the installation vault origin during new-member provisioning.
- [x] Route tokens by server-owned allocation scope; reject collisions and bound per-member capacity.
- [x] Bind lifecycle redelivery/Ack to the original owner and allocation; continue after an unavailable member.
- [x] Run Linux component checks, real-process member scenarios and independent review.

Linux: vault gateway 16 passed; lifecycle/onboarding 10 passed. Nine new gateway
scenarios cover invitation after startup, collisions, fences/restart, event
redelivery and mismatched Ack, event fencing, empty-member discovery/concurrent
admission, CLI mode constraints, per-member capacity and failed-Ack fairness.
The inheritance scenario also checks explicit overrides and invalid defaults.

The public stdlib-only `deployment/kiri/linux-vm-smoke/member-vault.py` ran real
release gateway, two private member stores and the vault service on Linux:
MV-01 through MV-06 passed. It checked a member added after startup, single-use
handoffs, both directions of account isolation, logout isolation, gateway
restart/fresh handoff and registry fencing. It also confirmed the old Google
owner helper is denied in installation-wide mode. These are real processes and
HTTP/Unix requests with synthetic identities, not browser UI or separate UID
container acceptance. Initial harness failures (HTTP helper name shadowing and
missing member header) were fixed before the six passing scenarios.

Independent Claude Sonnet 5.5 / medium found a pending lifecycle Ack could starve
other members. The routing cache now expires after 30 seconds, drops on upstream
Ack failure, and resumes discovery; a Linux scenario confirms the other member
progresses and the original durable event remains retryable. Its key-permission
migration concern was checked against the base: 0400/no-create was already the
implementation and had existing regression coverage; only stale CLI help changed.

The gateway release was deployed in the existing fixed-member mode. The public
shell, vault health and executing binary were verified; the member image,
registry schema and Google helper were not changed. The installation-wide mode
is available and tested, but enabling it in production awaits the member-aware
Google browser/recipient transport. A shared vault origin does not enroll a
Google recipient or make OAuth/GWS ready. The overall clean-install gates above
remain open.


## 2026-11-26 public browser prerequisite

The public Rust capsule launcher now selects fixed local Docker or Kiri sockets.
Kiri requires the observed 0.2.2 component, the current reviewed seccomp profile,
and ephemeral workspaces. The old Docker configuration serialization is retained.
The production worker checks actual read-only mounts, noexec/nosuid writable
mounts, clone3 denial and cgroup-v2 memory/PID/CPU limits before browser launch.
This is the public Chromium research worker, not the Google sign-in Chrome runtime.

Linux acceptance used the same test image
`sha256:65e60d0682fdab3f0fff308e123f3204965a6a36cd2ad0a1d2107473c3185207`
on host Docker and fresh Ubuntu 24.04.5 / kernel 6.8.0-147 / Kiri 0.2.2.
The official Kiri amd64 package SHA-256 was
`24798125ee50bb24db86594a9ee1469b1032d9b2ffd7da616b06e40763c723b9`.
The Kiri VM had no shared host files, production data or credentials. Ubuntu's
application-specific Chromium user-namespace policy was installed in that VM;
no global AppArmor or namespace restriction was disabled.

| Scenario | Docker | Kiri 0.2.2 |
| --- | --- | --- |
| KL-01 public Rust launcher renders the owned fixture | passed | passed |
| KL-02 two members have separate pages; third worker exceeds capacity | passed | passed |
| KL-03 one member shuts down while the other remains usable | passed | passed |
| KL-04 stopped slots can be reused and all owned workers removed | passed | passed |
| Worker JS/negation/PNG and stale/cross-worker handles | passed | passed |
| Private and loopback live canaries, DNS rebinding, direct egress denial | passed | passed |
| Worker-only cookie recovery, blank restore and form-value omission | passed | passed |

Two earlier Docker launcher attempts failed with Timeout/CleanupUnconfirmed.
A bounded diagnostic captured synthetic worker readiness and rendered output,
then all four launcher scenarios passed with clean owned inventory. The final
complete Docker smoke also passed. A subsequent synthetic diagnostic exposed Docker auto-removal racing the
launcher. Ready workers now receive up to 2.5 seconds of graceful shutdown and
failed immutable-ID removals receive bounded absence reconciliation instead of
failing after 200 ms. Never-ready cancellation retains its earlier short bound.
The diagnostic does not identify every earlier timeout or establish reliability
under load. Kiri's final complete smoke passed. Rust Kiri profile recovery remains
explicitly refused despite the lower-level worker cookie test passing.

Linux browser coverage now has 50 cases. The first revised run passed 49 and
exposed a cancellation deadline regression; retaining the short bound before
readiness fixed it. All 29 affected capsule tests then passed, including delayed
auto-removal and Kiri recovery rejection. Other browser tests were unchanged. Browser relay: 7 passed. Harness regressions: 5 passed.
`verify-plan` passed. Changed Rust files were formatted; full-workspace formatting
has pre-existing differences outside this unit. Security review/publication status
is recorded after the final review below. No production engine upgrade occurred.


Final uninstrumented Docker and Kiri smoke passed after the shutdown correction.
Observed AppArmor labels: Docker worker and Chromium `docker-default (enforce)`;
Kiri worker `unconfined`, Chromium `moosuri-chromium (unconfined)`. These are
observations, not an image attestation or a claim of additional AppArmor file or
network confinement. Kernel namespace/capability/seccomp/cgroup and actual
network-denial checks remain the containment gates.

Independent Claude Sonnet 5.5 / medium reviews found and led to the engine-specific
profile gate, effective kernel checks, Kiri recovery rejection, explicit cgroup
namespace and corrected AppArmor scope. The final follow-up independently found
the same cancellation deadline regression; it was fixed in product behavior,
without relaxing the existing test. The reviewer found no high/medium defect in
the separately drafted member engine compatibility change; its VM acceptance is
still the next gate. Existing short Docker query deadlines can still fail closed
under host load, and pre-readiness cleanup uncertainty masks a primary startup
failure. Neither is treated as success or automatically retried through another
engine. The operator-asserted smoke hash is not a signed attestation. Existing
profile-path TOCTOU and load/reliability work remain outside this browser unit.

The browser prerequisite is published without a production Kiri upgrade.
Member provisioning on 0.2.2, official Google Chrome, portable recipient delivery,
clean installation packaging and actual OAuth/GWS acceptance remain open.
