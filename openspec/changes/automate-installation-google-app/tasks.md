# Work log

The current [42 user journeys](journeys.md) cover installation, Vault, Google login,
OAuth setup, sharing, conversation reads and recovery. They distinguish live
provider results, diagnostic installation evidence and open acceptance gates.

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
3. Member provisioning/lifecycle compatibility with Kiri 0.2.2 — implemented
   and accepted in a disposable Linux guest; existing allocation identities retained.
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

### Clean installation implementation — acceptance in progress

The source-only path is `deployment/kiri/install-from-source.sh` → Rust `xtask
kiri-host` / `kiri-bundle` → `moosuri setup-kiri`. It reuses the existing registry,
member storage, first invitation and authenticated administrator bootstrap.
The [source installation guide](../../../deployment/kiri/source-install.md)
records requirements and the still-open fresh-user gates. This implementation
is not production deployment or completed OAuth setup.

Initial Linux checks: 16 new installer tests and the existing installation/admin
tests compile; after enforcing the documented private test umask, 53 matching
unit tests and two integration cases passed. The ordinary guided-setup tests
initially rejected a 624 MiB debug executable at the production bundle size
limit; both passed using the same executable with debug sections removed.
Independent Claude Sonnet 5.5 / medium reviews found premature onboard success,
transient-login classification, cached package recovery, service readiness and
source-revision/attribute handling defects. The implementation addresses those
findings. The actual probe replaces the draft identity-only evidence hash.

| Installation scenario | Expected result | Acceptance |
| --- | --- | --- |
| CI-01 empty Ubuntu/public source | no prepared Kiri, Docker, GWS, app state or developer helper; build and install | running |
| CI-02 first administrator | HTTPS login works, initial password adopted through ordinary auth; role is admin | pending |
| CI-03 interruption/retry | same installation, invitation, UID, volume and adapter authority retained | pending |
| CI-04 conflicting inputs/artifact | changed origins or corrupt bundle refused without replacing member data | pending |
| CI-05 second member | installed timer prepares independent UID/volume; Vault and GWS inherit installation origins | pending |
| CI-06 dynamic Google recipient | each authenticated member gets a fresh scoped Kiri recipient; cancel/logout closes it | pending |
| CI-07 restart | installed services and isolated browser proxies recover; identities remain unchanged | pending |
| CI-08 exposure | synthetic password absent from receipt, argv and journals; cross-member Vault access refused | pending |

The first empty-VM execution stopped at tracked Vault asset symlinks before
installing dependencies. The two assets now contain the same bytes as regular
files. The next execution is building from its selected commit on the empty VM.

That source build completed, then actual installation found three integration
defects: the copied seccomp file retained group-write permission; Docker's new
containerd image store reported an index digest where Kiri needs the config
digest; and a newly created stopped proxy has its requested IP in IPAMConfig
but no actual IP until start. The builder now normalizes file modes and derives
the image ID from the saved config bytes; the installer checks the requested
address before start and the actual address afterward. Linux installer tests
passed 17 cases and the archive test covers both legacy and OCI save layouts,
wrong digests, path traversal and multiple image entries.

Applying the two packaging corrections to the disposable built bundle allowed
an intentional interruption and same-identity resume; that run exposed the
proxy check and is not complete installation acceptance. No member data was
created. A new source-built candidate must exercise all fixes automatically;
the manually corrected diagnostic bundle does not close CI-01.

Fresh-user completion additionally requires automatic shared model service and
member socket provisioning. The source installer currently prepares neither
`runtime-model` nor `model_sockets_root`; a web engine setting alone cannot close
that gate. Keep it as a separate usable unit after the Google path, then converge
the default public entry point. Do not treat this installer unit as full product
onboarding.

The supplied-certificate path now validates both HTTPS hostnames and validity
dates with the installed OpenSSL. A real Linux certificate scenario covers a
valid pair of names, each wrong name and expiry. Its first run caught OpenSSL
`x509 -checkhost` returning exit zero for a mismatch; the implementation uses
`verify -verify_hostname` with the supplied certificate as the local trust
anchor. This is a consistency check, not a claim of external CA trust. The
empty-VM source build still uses the earlier candidate; this delta is not yet
included in its running bundle.

Next Google unit: branch within the existing signed-in attempt before a missing
client ends the job; reuse the registry's reviewed lease/intent/replay guards,
the authoritative atomic client writer and current GWS distribution. Keep the
SDK inside the disposable authentication image, with an empty tmpfs config,
fixed commands and exact account checks. Provider pages and the client secret
must remain outside model output and public receipts. Console selectors and
client-secret response-loss recovery still require actual discovery.

Separate unpublished next-unit drafts now contain the unbilled registry
checkpoint and a fixed-command Rust Cloud SDK runner. The checkpoint retains
used-run IDs, leases, fresh administrator checks and uncertain intents; billing
selection is removed and the order is project, APIs, branding, audience, client,
registration. Linux administrator/storage checks passed 22 cases, including nine
checkpoint scenarios. Legacy-schema fixtures were updated to remove the new
tables before their deliberate downgrade; no production migration guard was
relaxed. The SDK's nine synthetic cases passed, but browser/controller integration
and real provider operations are still missing.

The versioned official SDK 594.0.1 archive is separately pinned to SHA-256
`8fc798fd0638428551f80277a568112d08afcf9cafd967d1cf39ae0918df2a1d`;
the older unversioned archive's hash below must not be substituted. Inspection
of that SDK confirms `--no-launch-browser` uses an auth-proxy/code flow. The new
candidate instead keeps the normal loopback flow and redirects the browser-open
request to the same private Chrome. SDK execution/URL compatibility in the image
is still under acceptance. No SDK credentials or Console client have been created
by these component tests.

Official documentation rechecked on 2026-11-26:
[SDK login](https://docs.cloud.google.com/sdk/gcloud/reference/auth/login)
supports browser authorization with controlled launch; its precise callback mode
must match the pinned SDK. [Project creation](https://docs.cloud.google.com/sdk/gcloud/reference/projects/create)
supports a caller-selected stable project ID and labels without requiring a
parent by default. [OAuth best practices](https://developers.google.com/identity/protocols/oauth2/resources/best-practices#manual-creation-and-configuration-of-oauth-clients)
requires Cloud Console client creation/configuration and explicit terms; do not
invent an unsupported client-creation API. These docs do not prove that the
unobserved Console steps or a never-used Google account already work.

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


## 2026-11-26 member engine compatibility

Member provisioning accepts exactly Linux Kiri 0.2.1 or 0.2.2 / API 1.47.
The historical `kiri-0.2.1-none-v1` allocation label identifies the unchanged
storage/UID contract; it is not the currently running engine version. The
existing image, UID, volume, generation, mount and isolation comparisons remain.
The public browser additionally requires 0.2.2 and its custom seccomp profile.

The first blank member installation exposed a cold-image create taking about
14 seconds, beyond the original 10-second engine read timeout. The initialized
volume and created stopped container remained intact; a same-request retry
completed. The fix gives create/start responses 30 seconds, retains 10 seconds
for inspection, and re-inspects the original container after an uncertain
create/start result without repeating the write. Foreign or absent results
remain failures. Thirteen Linux provisioner tests passed, including uncertain
create/start recovery, foreign resource rejection and preserved allocation labels.

After removing only that synthetic installation and reloading its image, the
final source-integrated host binary ran the public `guest-web-invitations.py
--recovery` harness on Ubuntu 24.04.5 / Kiri 0.2.2. Eight onboarding scenarios
passed: authenticated invitations, stock timer/capacity waiting, repeat identity,
password rotation, retry/isolation, second UID/volume, installation conflict and
absence of passwords from the service journal/queue. Three recovery scenarios
passed: KR-01 unchanged reconciliation, KR-02 recover A while B keeps its PID,
and KR-03 daemon restart with the same IDs, complete inspected profiles, mounts
and member-bound login sessions. The test refuses to restart the engine if any
container outside those three synthetic members exists.

The member software image was the already built immutable
`sha256:14ee962e016b7f1e313da829f3966a0b1fb62fc8b3b5dd59643838e6232a902d`;
no running production container, member data or credentials were copied. These
are host provisioner/lifecycle checks with the existing member software, not a
public installer or a fresh end-to-end Google connection. A new installer must
build/obtain its member software through the public distribution instead.

Independent Claude Sonnet 5.5 / medium found no introduced authentication or
identity weakening in the Rust changes. Its fixture findings were addressed:
exact engine inventory before restart, full HostConfig/profile comparisons and
specific cross-member responses. This check does not claim a live production
0.2.1-to-0.2.2 upgrade, full guest reboot, cold-start load guarantees, OAuth setup
or signed artifact attestation. Production remains on 0.2.1 pending the portable
Google runtime and complete installation integration.

A read-only kernel probe through actual exec processes in the three recovered
member containers confirmed their distinct configured UIDs, zero capability
sets, NoNewPrivs=1 and Seccomp=2; read-only root; nodev/nosuid/noexec data, scratch
and IPC mounts; empty IPv4 route tables; and effective cgroup limits of 128 MiB,
64 PIDs and `25000 100000` CPU quota/period. This checks effective member-container
state on this guest, not arbitrary future Kiri releases or other host kernels.


## Portable Google authentication runtime

The public Rust worker and typed Kiri launcher now replace the private browser
modules at the **runtime component** boundary. They do not yet replace the
production gateway/owner helper or close the blank-install exit gate.

- [x] Guest-only RSA-3072 recipient with allocation/epoch/challenge/origin binding.
- [x] Vault ciphertext delivery and a separate authorized release before Chrome.
- [x] Official Chrome 158.0.8127.88 on private Xvfb/tmpfs with zero swap, bounded
  processes/memory, sandbox, no persistent profile or password manager.
- [x] Fixed account and consent operations; no general CDP/script protocol.
- [x] Cancellation, provider denial, wrong callback state and owned cleanup.
- [x] Linux real-browser fixture and independent Claude review.
- [x] Authenticated installation socket, gateway reservations and dynamic vault
  recipient integration; Linux member transport acceptance and independent review.
- [ ] Same-browser SDK/Console setup, public installer, real two-member Google
  consent/GWS read and matched production deployment.

Final runtime evidence on 2026-11-26: Linux Rust browser suite **57 passed**,
including real Node WebCrypto/Rust interoperability and browser callback script
execution. `google-auth-smoke.py` passed GR-01–10 using Docker and the real Rust
Kiri launcher on Ubuntu 24.04.5/kernel 6.8/Kiri 0.2.2. Only synthetic accounts,
passwords and TLS provider responses were used. The test proxy cannot forward
to Google or the public internet.

| Scenario | Observed result |
| --- | --- |
| GR-01 | Ciphertext to official Chrome login and same-profile consent |
| GR-02 | Device-verification wait and continuation |
| GR-03/04 | Mismatched account and rejected browser receive no password |
| GR-05 | Changed allocation cannot decrypt or start Chrome |
| GR-06 | Stale challenge cannot release Chrome |
| GR-07 | Cancelling one member preserves a sibling attempt |
| GR-08 | Exact provider effects and no remaining owned authentication container |
| GR-09 | EOF and SIGTERM abort before password delivery |
| GR-10 | Denied and wrong-state callbacks never report success |

The fixture caught a duplicate JavaScript declaration in callback checking and
an earlier Tokio stdin shutdown hang; both were fixed. The launcher fixture
initially passed a lab-user-owned seccomp file to a root process, correctly
failing closed. It now creates an owner-only fixture copy. Docker can close its
attach stream on SIGTERM without a receipt; the test checks nonzero exit and
container removal, never interpreting EOF as successful teardown.

Test image: `sha256:3a3bdbfc07ffe865019d7e50a1c11799ba1d9088ce8a161ed7ad7a7bbbf229ef`.
The official Chrome Debian package is pinned by version and SHA-256 in the
Dockerfile. Images are built from public source; no image was pushed or deployed.

An initial large Claude review timed out. A bounded independent Claude Sonnet
5.5 / medium review of the launcher, worker, crypto, Chrome adapter and callback
script found no introduced high/medium defects. Earlier reviews covered the
remaining confinement/account/consent code. Low-priority diagnostic masking and
conservative timeout observations are retained for later troubleshooting; they
do not establish or block provider acceptance. Review does not cover the later
unpublished gateway/Vault draft.

Ubuntu already provided `/etc/apparmor.d/chrome`. Loading another differently
named profile for the same executable caused a userns transition denial. Reuse
that distro profile; `apparmor.chrome` is a fallback only. This is host path-based
userns admission, not image attestation or a file/network sandbox.


## Portable gateway and Vault integration — 2026-11-26

The installation-owned socket now creates a guest-only recipient for the current
member allocation and epoch. The gateway exposes `preparing` before `vault`, so
the web client keeps its popup until the recipient is ready. The Vault service
uses its existing authenticated adapter to retrieve exactly that challenge;
legacy fixed-recipient deployments retain their earlier protocol. Credentials
are sealed in the vault browser and only ciphertext crosses the gateway.

Root peer credentials, installation/origin, allocation and epoch are checked.
The same checks precede password release. New members have epoch zero; accepting
that actual registry state fixed a fresh-member rejection without treating missing
or unreadable registry records as zero. Only the installation-owned client file
can select the OAuth identity. The existing callback remains the grant authority.

Linux real-process/Kiri acceptance used the public `member-google.py`, two new
member stores, the gateway, vault service and socket-activated-style brokers with
official Chrome against an isolated synthetic TLS provider. **MG-01–10 passed**:

| Case | Observed result |
| --- | --- |
| MG-01 | Concurrent new members receive separate guest-only recipients |
| MG-02 | Cross-member recipient lookup is refused both ways |
| MG-03 | Cancelling A invalidates its key while B stays usable |
| MG-04 | One synthetic login; missing installation app remains `setup_required` |
| MG-05 | Identical Deliver replay is idempotent; changed ciphertext is refused |
| MG-06 | Gateway shutdown removes owned auth capsules; exact provider effects |
| MG-07 | Immediate retry creates a fresh key; broker termination invalidates it |
| MG-08 | Logout invalidates vault access and reclaims the idle capsule |
| MG-09 | One preparing/prepared key per member preserves sibling capacity |
| MG-10 | Cancelled preparations consume the per-member budget across logout/login |

Expected and observed provider effects were one identifier and one password,
zero consent, callback or wrong-password submissions. Only the fixture proxy
remained before its final cleanup. This is not actual Google, browser UI,
separate member UID, OAuth app creation, public installation or production evidence.
Test image: `sha256:a0ecd2a4a3167763b5b3168d6a4ec57f947a1561368c198e66e74292958afdcf`.

Independent Claude Sonnet 5.5 / medium reviews led to cancellation-fence fixes,
one prepared key per member, a four-start/minute budget, bounded/revocable
startup, expiry-independent idle cleanup, broker-death detection and a shared
root-owned startup flock across broker processes. A trial child-exit monitor
closed stdin prematurely; the real fixture and reviewer caught it. It now polls
`try_wait`, preserving later delivery. The final delta review found no remaining
high/medium defect. Existing engine inventory also caps auth capsules at two.
Cleanup tolerates an already-reaped child; immutable IDs and ownership checks
prevent a losing startup from removing another attempt. Unconfirmed removal
fails closed; heavy-load availability and forced-kill orphan recovery remain
limitations, not proof of successful cleanup.

Linux regression evidence: gateway suite 58, then affected Google cases 27;
auth crypto/script cases 7; vault HTTP 2 under private umask; Vault web 51; web
connection/Workspace 34, then connection cases 21 after preparing-state coverage;
latest capsule regressions 29. Relevant UI typechecks/builds passed. The first
vault HTTP run failed with a permissive fixture umask; production starts with a
private umask and no vault permission guard was weakened. Two later fixture
failures used a post-login teardown fence and a pre-rotation synthetic password;
fixture correction preserved those product guards. Planning/OpenSpec and secret
checks passed. No dependency or license changed.

This unit does not switch production or the public installer. Next: build all
installation artifacts from public source, provision the Linux services, then
implement same-profile SDK/Console app creation and actual member consent/GWS.
The conservative post-login cleanup fence is retained; only key-only preparation
can retry immediately. Overall clean-install and real-provider gates remain open.


### Clean-install recovery review — 2026-11-26

The initial complete source build exposed artifact-mode and Docker OCI config-ID
compatibility failures, followed by the stopped proxy pre-start address check.
Candidate `00c61ed` fixed those issues. The diagnostic repaired bundle is not a
clean-source acceptance result. No product data was initialized in that run.

Independent Claude Sonnet 5.5 / medium review also identified late TLS validation
and incomplete mode normalization. The candidate now validates both HTTPS names,
dates and certificate/private-key pairing before saving a receipt; encrypted keys
and nginx-token path characters are rejected. Image tars and the manifest receive
explicit private modes and metadata checks. A stopped proxy may have empty endpoint
IDs, but its requested IP and its exact post-start internal/public IDs are checked.

Linux: 21 installer tests and 2 bundle tests passed, including real OpenSSL good,
wrong-host, expired, wrong-key and encrypted-key cases. Initial failures in the new
fixtures (shared cert modes and a missing fake inspection response) were fixed
without weakening product validation. Closure review found no unresolved high or
medium defect in these boundaries. The probe-resume concern was resolved by reading
the actual probe: published evidence/config is deterministic for unchanged resource
identities, so the existing re-probe and exact-byte comparison are retained.

Pending: an unmodified source installation, real post-probe interruption/resume,
installed-member/Vault/invitation scenarios and reboot. Production and the default
public installer remain unchanged; OAuth issuance and real Gmail read remain open.

### Host startup correction — 2026-11-27

Rebooting the empty-data Ubuntu guest exposed a real startup race: Kiri's default
loopback HTTP/HTTPS routers occupied ports 80/443 before nginx. The host installer
now uses Kiri 0.2.2's supported `/kiri/domains` API to persist 18780/18743 before
installing/starting nginx. A changed port plan requires an empty engine; replaying
an already-matching plan finishes persistence without restarting containers.
The read waits boundedly for the engine socket. Actual Ubuntu curl testing showed
that `--retry-connrefused` did not retry an absent Unix socket, so read-only retries
use `--retry-all-errors`. JSON contract mismatches still fail immediately.

Linux `xtask` compilation passed. Independent Claude Sonnet 5.5 / medium review
found no definite material defect after startup ordering was repaired; its absent
socket observation was reproduced and fixed. The unmodified source installation
at `adc8be7` passed host preparation and is building all artifacts in the disposable
VM. Later host-only revisions are `0429b1c` and `4c246eb`. Completed installation,
post-probe crash/resume, installed member/Vault scenarios and reboot are not yet
claimed. Production and public main remain unchanged.

### Empty-server acceptance findings — 2026-11-27

The controlling user journey starts with another person's empty Ubuntu server.
Existing Mac credentials, developer helpers, owner routing, model worker config,
Cloud projects and OAuth clients are not installation prerequisites or acceptance
evidence. OS installation may ask for the administrator identity, public origins
and TLS choice; provider credentials belong to the later authenticated setup.

The `adc8be7` source build completed all artifacts. Actual Chromium startup then
failed because Ubuntu's AppArmor policy for Chromium was not loaded by the host
tool. The host tool now installs and loads the repository's exact Chromium policy
alongside the official Chrome policy, without weakening the host sandbox. Linux
build and independent Claude Sonnet 5.5 / medium review passed. Using that updated
host tool with the already-built bundle allowed both real browser probes to pass.
This mixed diagnostic run does not close unmodified-source CI-01.

After both probe receipts were saved, the exact installer process was stopped and
killed, then resumed with the same bundle, inputs and password. It retained the
installation identity and created the first member. Admin promotion then failed:
the installer reused the member-allocation request ID for a different registry
operation. Repeated retries hid the conflict behind HTTP 429. The correction uses
a stable, domain-separated administrator request derived from the durable receipt
and retries only startup failures; rate limiting is explicit and terminal. The
first-admin fix is independently reviewed. Both the stable-request registry case
and the real private-HTTP temporary-password adoption/replay/429 case passed on
Linux. The later installer suite passed 22 cases.

The diagnostic guest was completed using the existing authenticated bootstrap
command with the corrected request identity, retaining the original bundle and
all member state. An attempt to run a different installer executable against that
bundle was refused; no artifact integrity guard was weakened. The next real check
found the gateway crash-looping despite a momentary systemd active state: systemd
reset the proxy runtime directory's group between the pre-command and ExecStart.
The unit now declares its group directly. Both public HTTPS endpoints must respond
before completion is recorded, including completed-install replay. Independent
Claude review found no blocker; curl is included in the preflight tool list.

With those explicit diagnostic corrections, installed CI-02/03/04/05/06/08 passed:
HTTPS administrator and single-use Vault handoff; exact replay and conflict refusal;
timer-prepared second UID/volume with GWS 0.22.0; separate real Kiri recipients;
cross-member refusal, cancel and logout; no synthetic secret in argv/journal/receipt.
A real reboot passed CI-07: the receipt, member enrollment, adapter key and both
browser evidence files retained their hashes, and HTTPS admin/Vault recovered.
Post-probe crash/resume retained the original adapter key and both probe receipts.
This is recovery/component evidence. **CI-01 remains open until the final source
revision installs on a new empty VM without any diagnostic correction.**

Model onboarding remains a separate required usable unit. A source-only audit
confirmed that the web engine page selects an already-published catalog; it cannot
create the first provider connection. `runtime-model` is the only shared inference
service and currently loads fixed member bindings. Implement dynamic enrollment
using the existing lifecycle member records and registry engine state, start it
unconfigured without credentials, and expose first provider registration to the
authenticated administrator. Add the service, stable fingerprint key, member
socket mounts and text-preview capability to the same installer. A new registry
or a second inference framework is unnecessary. Validate first-member chat,
second-member automatic attachment, disable/change, restart, generation fencing
and provider failure with Claude Sonnet 5.5 / medium. Public-installer convergence
and a clean-source rerun remain required after integration.

### Private SDK and same-profile callback — 2026-11-27

The pinned SDK uses an empty private configuration inside the Google-auth capsule.
Its loopback callback is intercepted in the same Chrome profile that signed into
Google, validated by session/top frame/method/state, then delivered over a fixed
local TCP connection. Successful capture closes the intercepted tab and opens a
new tab in that same profile; it never disables Fetch, which could resume an
unaccounted paused request. Failed/cancelled attempts taint the browser and cannot
be retried in place. The normal proxy still refuses private destinations.

Linux component checks: 23 authentication tests and 4 CDP tests passed. Actual
pinned Chrome in the confined, network-disabled image passed valid capture,
second capture in the same profile, wrong state, iframe, popup, subresource,
burst and retry-after-error cases. The public `sdk-browser-smoke.py` harness
runs failing cases in separate capsules. An initial fixture incorrectly tried
to restart a one-use X display in the same capsule; that fixture was corrected.

The real SDK returns exit 1 when asked to revoke an empty account list. Cleanup
now first proves the private configuration is empty, or requires successful
revocation followed by a fresh empty list. A failed revoke remains a failure
even if it removed local credentials. The actual SDK's empty-config cleanup
was run twice successfully through the Rust runner. No grant or provider write
was made in this test. Independent Claude Sonnet 5.5 / medium closure review
found no supported high/medium defect in this component.

Still open: parent controller/worker integration with cooperative cancellation,
at least 65 seconds for SDK cleanup before container removal, fresh administrator
heartbeats, provider project reconciliation, Console branding/audience/client,
private atomic client registration, member consent/GWS, and a real Gmail read.
These component results do not establish an automatic Google setup or deployment.

The SDK now probes the exact stable project ID with a fixed `projects list`
filter: only a successful empty list means absent; transport errors, malformed
results and foreign identities never permit a create. A login handle can also
be explicitly killed and reaped before credential cleanup. Linux SDK checks:
20 passed in a serial run. The initial stop fixture spawned a shell child rather
than execing the login process and counted killed zombies as running; it now
matches the SDK wrapper's exec behavior. One parallel run reported a transient
spawn failure with no captured OS cause; the focused stop check and full serial
suite passed. Actual provider creation/reconciliation remains untested in this
new product path. Worker and capsule integration is the next implementation gate.

### Same-browser SDK preparation foundation — 2026-11-27

The private worker/controller now carries fixed typed SDK actions after the Vault
Google login, with a stable project identity, administrator heartbeat and no CLI
or CDP input from a member. The authenticated recipient owns the run identity.
The controller persists SDK intent before dispatch. A failed or killed cleanup
leaves a durable fence across lease expiry. Proven root-broker teardown records
every closed browser run, including before a late claim/begin commits, and never
clears a successor. Both systemd service templates allow the SDK cleanup grace.
No HTTP or operator command bypasses this fence.

Independent Claude reviews found and led to fixes for lost cleanup proof, lease-only
recovery, late start/claim after teardown, mismatched run IDs and unused-SDK exit
classification. Linux registry cases: 16 passed; relay: 2 passed; controller: 4
passed. Worker/SDK suite: 36 passed, 1 ignored, 1 project-probe fixture failed; that
single failed case passed on its focused rerun. Its transient cause is unproven,
so the broad run is not described as fully green. Earlier affected capsule cases
passed 31. Real Kiri/tini 0, 7 and SIGKILL/137 exit propagation was checked separately.

This foundation is not called by the user connection route yet. Console branding,
External audience, web client creation/secret recovery, atomic shared registration,
same-profile service consent and real Gmail read remain open. The research login
expired at Google device verification; there is no live authenticated research
session to reuse. Actual SDK revoke-failure/forced-kill acceptance and recovery from
truly unknown remote revocation also remain open. Production and public main are
unchanged; migration 8 and these checkpoint fields have never been deployed.


### Live Flip5, installation client and Gmail — 2026-11-27

Phone Agent MCP on Linux confirmed the connected Galaxy Z Flip5, permitted work,
and reserved the UI. The operator approved only the fresh matching Google/Linux
request. A fresh Kiri research session reached authenticated Console; it was
closed after the experiment. The phone reservation was released.

In the task's newly created unbilled project, the observed Console wizard created
Moosuri branding with External audience. The new web client uses the actual
installation HTTPS callback and the AI-powered-agent marker. Its one-time secret
was captured privately on Linux, then supplied through the real Kiri-hosted
Moosuri administrator file-input/form. The existing registration endpoint returned
configured=true and the UI confirmed storage. Scope configuration used Add to
table, Update and Save; one authorized account was registered as a test user.
The app remains Testing and is not production verified.

The actual member Google connection was then started again from its UI. A wrong
Vault password remained locked; the correct one reused the existing Google login
without requesting that Google password again. After a new Flip5 confirmation,
the same product Kiri browser completed service consent and the app showed
connected. The member's real GWS-backed inbox API returned HTTP200/completed,
five matching-account metadata items and has_more=true. A reload displayed one
saved receipt and five rows. No mail subject/sender/body or credential was logged.

The ordinary composer request “최근 받은 메일 5개 확인해줘” failed at work
interpretation and retained its draft. The subsequent API read is not evidence
that this natural-language path works. The classifier's closed contract currently
contains local work and preferences but no Gmail inbox operation; determine the
actual model error and integrate the existing read path without a parallel router.

These are existing-installation/provider results. Public main, binaries and Kiri
version are unchanged; production now has the shared OAuth client and the tested
member Gmail grant. Portable Console automation, secret-rotation recovery, final
empty-source installation and real two-member acceptance remain open.

## 2026-11-27 journey correction tranche — not deployed

The [49 journeys](journeys.md#2026-11-27-candidate-correction-evidence) now link
candidate corrections and distinguish synthetic, live operator and final-install
acceptance. The main still owns final integration and release.

- Dynamic member model sockets, admin-only API setup/CAS replacement and private
  state outside the registry are implemented; Linux runtime, installer and
  role-boundary tests passed. Independent Claude Sonnet 5.5/medium review found
  registry contamination, one-member fault amplification and failed-key retry
  defects; they were corrected and reviewed again. One follow-up UI attribution
  defect (another admin's save after a lost reply) is corrected with a regression
  scenario. Real API registration and the missing production subscription worker
  are not verified by these checks.
- Typed conversational inbox dispatch now reuses the existing interpretation
  journal and Gmail read ID. Positive request grammar, stale-thread guards,
  Vault/reload continuation and explicit fresh retry after uncertain results are
  implemented and pass focused Linux tests. Real natural-language completion is
  still blocked by the absent production worker.
- Actual Kiri Console readback exposed and corrected account shadow DOM,
  editable branding and abbreviated scope selectors. External/Testing audience
  eligibility has typed inspect/configure actions. The public worker/controller
  still needs complete durable substep integration and client issuance/registration
  before the existing member route can use it.

Two original test failures were fixture errors, not acceptance: inherited Linux
umask 0002 made the synthetic API-key directory group-writable; its fixture now
sets 0700 explicitly. The cold-volume replay fixture reserved 2048 tokens for a
2370-byte-token member envelope; it now uses the broker's 4096 hint and asserts
that the encoded request fits. The important outcome (one dispatch/charge across
suspension, response loss and cold-volume restoration) passes.

## 2026-11-27 integrated client and recovery checkpoint — not deployed

| Usable unit | Implementation/integration | Validation | Public/production |
| --- | --- | --- | --- |
| One-browser first Google setup | SDK, Console journal, client inspection/creation and atomic private registration connected to the existing member route | Synthetic Console + real gateway authorization/registry/writer passed; actual client DOM/create open | Candidate only |
| Interrupted setup recovery | One provider dispatch; three local save attempts; exact saved-file reconciliation; retained cleanup fence | Save failure/lost acknowledgement/logout and registry boundaries passed; actual secret rotation open | Candidate only |
| Connection UI lifetime | 20-minute first setup, transient status retries, bounded GWS wait and truthful terminal states | Linux UI/DOM passed; final full journey open | Candidate only |
| Conversational Gmail + first model | Typed interpreter/request reuse and dynamic API model setup integrated | Component and isolated classifier calls retained; missing production worker still blocks ordinary live request | Candidate only |
| Shared app for later members | External audience chosen; limited-audience publishing requirement added to design | Actual app remains Testing; production transition, two real members and seven-day recovery open | Not ready |
| Final portable installation/release | Installer/lifecycle corrections retained | Final-source clean install, reboot and matched rollout open | No partial rollout |

Independent implementation and two recovery reviews used the official Linux
Claude CLI with observed `claude-sonnet-5-5`, medium. The main integrated and
verified the changes; delegates had no provider credentials or live browser
contents. Fixed findings: premature teardown acknowledgement, lost secret on
transient save, UI cancellation on one poll failure, skeleton-list false absence,
ambiguous result-dialog selection and nonzeroizing relay buffers. The stable
client name matches the registry's simple setup UUID.

The closing review found another NotFound ambiguity in registry authority
checks; only the private-file callback now classifies a lost secret. GWS waits
are measured from signed-in/Finish and each request is bounded. An ambiguous
OAuth-start POST is intentionally not retried: it may already have created
state. The suggested broad 5xx retry was not adopted. A pin expiring under severe
registry contention still stops the operation; Forbidden is not retried as a
storage error. Exhausted saves or process loss remain the open O05 rotation
journey, not a claim of arbitrary crash recovery.

Linux evidence before the final classification/deadline refinement: gateway
Google connection 36 passed; registry Google bootstrap 21; connector browser
122 passed/1 ignored; Kiri connection UI 32; client DOM 7. The earlier concurrent
synthetic SDK probe failed once, then passed isolated and serialized reruns;
no live SDK success is inferred. Final refinement and source/plan checks are
recorded below when completed. Tests use synthetic identities and temporary DBs.

A fresh operator Kiri sign-in returned `needs_human/provider_sign_in_required`.
No authenticated session or new Google resource was created. Phone Agent
reservation was released without approving a login. Resume actual client
selectors/creation/rotation via the authorized Vault-backed fresh Kiri path;
never reuse stopped operator sessions.

Independent model investigation confirmed that the public installer does not
provision the missing subscription worker. Existing official login/worker code
pins one principal/member per rootless worker; shared installation use goes
through the Anthropic API connection. M01–M06 cover first setup, later members,
bad credentials, lost replies, fresh subscription login and reboot. S06 covers
External production status and long-lived use. The matrix has 49 journeys;
component totals do not close their end-to-end acceptance.

Next: actual client/publishing/rotation observation and integration; model
first-use and real conversation; final-source empty Linux install/two members/
reboot; matched publication and production rollout.

Final candidate refinement: Linux gateway cases 36 passed; root relay 2 passed;
engine setup/settings UI 17 passed; web typecheck and production build passed;
`xtask verify-plan` passed with 1,749 local links. The API connection recovery
form now remains available after successful initial configuration, so a revoked
key can be replaced by the administrator. No provider request occurs merely
from opening that form.
The pinned OpenSpec strict validation of this change also passed on Linux.

Source checkpoint checks: `git diff --check` passed. Static Gitleaks directory
scan reported one pre-existing incomplete synthetic PEM marker in the installer
path-validation fixture (body `AAAA`, no end marker or usable key); it was
manually matched to the existing documented historical finding. No new scanner
exception was added. Lockfiles and third-party dependencies/assets are unchanged;
the new DOM fixtures are synthetic, not copied provider pages. Runtime tests and
UI build ran on Linux; the static source scan ran on the Mac checkout. This is a
local recovery checkpoint, not publication or final license/release acceptance.
