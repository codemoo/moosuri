# Work log

The current [61 user journeys](journeys.md) cover installation, Vault, Google login,
OAuth setup, sharing, conversation reads and recovery. They distinguish live
provider results, diagnostic installation evidence and open acceptance gates.

Latest: public dee84754 empty-start terminal install, retained-state storage
recovery, second-member web credential completion, two browser/Vault journeys,
actual reboot and egress isolation passed. The same bundle is deployed with the
existing real member/Vault/keys/Google grants preserved; post-reload GWS read
returned five metadata rows. Natural-language Gmail and fresh automatic Google
setup remain open. [Exact current evidence and limits](../../../docs/reviews/2026-11-27-final-entry-production.md).

- [x] Reconcile scope with a clean public installation and no pre-existing Google resources.
- [x] Package installation/runtime/browser/vault/GWS dependencies without developer files or fixed owner routing; corrected source installation and two-member reboot accepted below.
- [ ] Observe official Console steps in fresh Kiri and identify provider prerequisites.
- [ ] Implement resumable administrator setup and direct client registration.
- [ ] Integrate same-browser member consent and automatic GWS readiness.
- [ ] Validate Linux scenarios, obtain independent Claude review, deploy and publish.
- [ ] Complete real OAuth registration, member consent and one GWS Gmail read.

Implementation order within the public-installation gate (do not treat an
individual prerequisite as completion of Google setup):

1. Member-aware vault enrollment and lifecycle — published and deployed with
   the portable Google transport; final real multi-member acceptance remains open.
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

## 2026-11-27 live Console correction and fresh installation run

- O02: the integrated fixed client script now reads the actual existing Console
  list/detail and fills the create form up to, but not including, Create. The
  draft was cancelled and the fresh Kiri session stopped. Fixed header checkbox
  alignment, editable Name/type heading and a hidden loading placeholder.
  Linux: 8 Rust client cases and 13 DOM fixtures passed. Actual creation-result
  and rotation remain open; no extra OAuth client or secret was issued.
- O07/S06: actual publishing is disabled until Branding's public information is
  complete. Candidate `/about`, `/privacy`, `/terms` routes and pages are added.
  The real Linux browser covered anonymous direct entry, links/reload,
  keyboard/layout and no member/API/provider requests at 390/1280px. Gateway
  boundary test and 5 UI cases passed, as did typecheck/build. Independent
  Sonnet 5.5/medium review identified the omitted Calendar permission; it and
  the separate mail-write consent are now disclosed against actual source.
  Automatic domain/publishing lifecycle integration is in progress, not live
  acceptance or deployment. [Detailed evidence](journeys.md#subsequent-live-console-and-public-page-acceptance)
- I01–I06: another pristine Ubuntu guest is installing immutable candidate
  `dabc8c9d13604ef48208e9d55295b0d95fe9a687` from a Git bundle with no preinstalled
  Docker/Kiri/compiler/Google credentials. Its web/Vault/server build passed;
  runtime image preparation and installed acceptance are still running. This
  source predates the latest Console/public-page corrections, so it cannot close
  final-source release acceptance. Keep the new final-install guest; the older
  stopped diagnostic disk was compressed off-host and its restored SHA256
  verified before removing only that old disk to preserve host capacity.
- M01/M05: real shared API model acceptance requires an authorized API connection.
  A secure administrator-screen registration question is pending. Existing
  developer Claude CLI authentication is used only for bounded development
  verification/review, never copied into the installed product or shared between
  members. Other installation and Google work continues independently.

### Publication integration and clean-install failure — 2026-11-27

The candidate now journals Domains and Publish separately after the private OAuth
client save. Old Registered records do not establish publication. Managed setup
blocks all seven consent-start routes until publication is confirmed, including
a saved client whose checkpoint acknowledgement was lost. Unreadable registry
state fails closed; legacy manually registered installations remain distinct.
Linux registry40 and gateway38 cases passed, plus the actual HTTP start-route
and unreadable-registry regression. Independent Claude Sonnet 5.5/medium review
of this frozen integration is running; provider publication remains unverified.

The fresh Ubuntu VM's source install at dabc8c9 failed while compiling the member
Docker image: the Docker context excluded the embedded model-service template.
Added that exact public service file to the existing Docker context allowlist.
No product services or member state had been installed. Continue from the same
VM with the corrected integrated source; record this as interrupted-install
recovery until a complete final-source install and reboot are observed.

One synthetic SDK project-probe test returned Unavailable during the first
concurrent connector suite. Its isolated SDK suite and six bounded full Google
connector runs under Linux process tracing passed. No ETXTBSY/EAGAIN/ENOENT
spawn failure was observed. This does not establish the original cause; no
speculative production process retry was added. Retain the intermittent failure
as a diagnostic limitation and proceed with installation/provider acceptance.

Independent publication review completed on Claude Sonnet 5.5/medium. It found no
definite authorization/storage defect, but identified a practical dead end when
publication intent committed before dispatch. The candidate now permits only
Domains/Publish to retry after fresh exact Absent readback, bounded to three
durable attempts across restarts. Resource creation is unchanged. A separate
fresh independent review of this delta found no defect. Updated Linux registry
suite41 and publication/recovery cases passed. Existing manual-client callback
ownership/resume regression passed with test-fixtures enabled.

Revision `116a8cdce3b9b8ff28e5a26377658292076f94b6` contains the runtime,
public information and Docker-context corrections. It is committed only in the
candidate and is being installed in the same marked Ubuntu VM with no product
state. Public main and production remain unchanged. The original domain
coverage and unverified-consent questions remain live provider gates; the
review did not verify Google behavior.

A fresh operator login for follow-up consent inspection expired at device
verification. The matching Linux/Seoul/current request was located on the Flip5;
by the time its confirmation was delivered the browser returned
`needs_human/verification`. No authenticated session was published. The helper
discarded its temporary profile and the Phone Agent reservation was released.
No Google project/client/publication or GWS configuration changed. This does
not validate the candidate's packaged login or consent path.

### I07 current-host transition preflight

Read-only production inventory found Ubuntu 22.04.5/kernel5.15.0-179 and Kiri0.2.1,
with existing versioned product services and no setup-kiri receipt. The new
Ubuntu 24.04 installer intentionally refuses this existing installation. An
independent Claude Sonnet 5.5/medium source review confirmed that setup-kiri and
kiri-host are fresh-install/resume tools, not existing-installation updaters.
Do not forge a receipt or relax those guards for rollout. Preserve existing
member/volume/key identities, encrypted Vault data and Google grants; retain
production service names while switching a matched bundle. Kiri engine and
browser confinement compatibility on the current kernel, guarded member image
replacement and browser capsule preparation need separate transition acceptance.
This is not evidence that Kiri 0.2.2 fails on the current kernel. The approved
product deployment does not require or authorize an unrelated host OS upgrade.

After the corrected installation finishes, the queued acceptance pipeline runs
the actual HTTPS/admin/Vault/two-member checks, real Chromium Vault UI journeys,
and a real guest reboot. The browser harness uses only a CONNECT tunnel to the
two marked guest origins; it does not mock API responses or call a provider.
Its results are pending, not yet a pass.

### Corrected installation and real Vault acceptance — 2026-11-27

Revision `116a8cdce3b9b8ff28e5a26377658292076f94b6` completed the source installer
in the same marked Ubuntu 24.04 guest after the failed earlier Docker build.
The guest had no product state before this successful run, but did retain the
earlier run's downloaded/build prerequisites. This is successful interrupted
source-install recovery, not an untouched final-revision baseline or a public
main deployment.

The shipped HTTPS services, administrator login, single-use Vault handoff,
same-input installer replay, conflicting-origin refusal, installed invitation
timer, independent second-member UID/storage/GWS, admin setup boundary, separate
real Kiri recipients, cross-member denial, cancellation and logout all passed.
The model service starts with no configured provider and refuses member setup
writes. Synthetic passwords were absent from process arguments, journals and
receipts. An actual reboot retained both member identities and service readiness.

The new [real Chromium harness](../../../deployment/kiri/linux-vm-smoke/clean-installed-browser.py)
uses only actual HTTPS services through a restricted CONNECT tunnel, without
response mocking. Its first run failed because an exact Playwright textarea
label also included the recovery key text; matching the stable label fixed the
harness. Both members then created an eight-character Vault, confirmed recovery,
saved an encrypted synthetic item, rejected a wrong password and reopened their
own item only at 1280px/390px. Seven-character setup was rejected. No plaintext
Vault password, recovery key or item secret appeared in Vault POST/PUT bodies.
After another actual reboot, both existing Vaults reopened with the same items
and isolation. Final screenshots were inspected at both widths; no horizontal
overflow. This does not close Google-account selection or real model journeys.

### Live client creation and secret-recovery observation — 2026-11-27

A fresh operator Kiri login succeeded after the matching Flip5 confirmation.
The product's fixed client DOM expression filled and submitted a distinct
disposable test client, captured the creation secret into a private Linux file,
and verified the persisted exact name, Web type, agentic marker and callback.
This proves the DOM subflow, not the installed gateway/SDK/registration path.
The operator used the existing experiment project, not a fresh product setup.

On that disposable client, `Add client secret` immediately added a second secret
without a confirmation dialog. The older secret stayed enabled. The new one
had a NEW marker and a rendered copy button containing its full accessible
label. The ordinary old-client view only showed masked values and explained
that lost secrets must be replaced. Same-client recovery implementation and
durable response-loss/two-secret checks remain open.

An operator inspection mistakenly emitted the disposable creation secret through
that copy button's accessible label. The generic inspection output now redacts
this secret pattern. The entire disposable client was deleted in Google and
confirmed absent by the product list expression, invalidating both test secrets;
its private local capture was removed. The original installation client and
member grants were not changed. Do not copy the secret or raw observation into
source, test fixtures or further prompts.

### 2026-11-27 same-client recovery and existing-member rollout

| Unit | Integrated result | Validation | Release state |
| --- | --- | --- | --- |
| Lost first OAuth secret | Fresh admin/lease + absent-file reservation, permanent same-client binding and three-attempt bound; one Add per worker; private capture and exact reload | Linux registry43/gateway38, added competing-save preservation, DOM17; actual disposable-client creation and recovery DOM subflow passed | Candidate, installed full path open |
| Existing-member runtime replacement | `kiri-rollout` reuses provisioner validation/locks; exact old container ID, same member/data/UID, no volume deletion, target replay/readiness | Linux provisioner20 and six actual same-kernel profile-rollout checks passed | Candidate, matched image/production switch open |
| Production-kernel browser admission | Kiri 0.2.2 on actual 5.15.0-179 Ubuntu 22.04 kernel | Real Chromium + Google Chrome installer probes and invitation/member/daemon recovery passed | No production engine/service change |

Recovery implementation and independent review used official Linux Claude
Sonnet 5.5/medium. Review found a missing competing-save acceptance case: it now
writes a different valid shared registration after recovery reservation, verifies
that recovery refuses it, and compares the file byte-for-byte. The existing
atomic writer already refuses replacement. Three pre-dispatch failures may still
exhaust the recovery budget; this conservative pause is documented, not reset.

Actual Console testing corrected NEW-in-DT and Enabled-plus-Disable markup.
No Disable control is clicked. After private capture, a reload kept the old row
unchanged and the new suffix matched; both two-secret states refused Add.
The disposable client was removed via Google's initial Delete and subsequent
name-bound DELETE confirmation, then the product expression proved absence.
The original installation app and grants were unchanged. Full installed recovery,
publication and two-real-member Google journeys remain open.

Independent rollout review found no blocking ownership defect. Its unproven Kiri
ID/stop/delete semantics were resolved by the actual disposable-guest rollout.
Documentation now distinguishes unchanged legacy reservations from optional
local-GWS mounts and limits reverse rollout to a matching reservation/profile.
Failures after a confirmed stop now explicitly report the retained stopped old
container; HTTP500 stop replies reconcile with inspection. Linux failure cases
cover those states without repeated stop/delete or implicit rollback.

Next usable units: matched final bundle/production component transition; real
managed setup/publishing; real two-member consent and conversational Gmail with
an administrator-provided model connection; final-source fresh installation and
coherent public release. Keep the 50-journey matrix open until those pass.

A fresh bounded Claude Sonnet 5.5/medium closure review read the actual private
writer/registry locks and the refined stop/remove paths and found no material
defect. The competing-save test passed, as did the final provisioner20 cases.
Linux OpenSpec strict validation and plan/link verification passed (1,754 links).
Static Gitleaks found only the previously documented unusable synthetic PEM
marker in the installer fixture; no new exception was added. Dependencies and
third-party assets are unchanged. Production/public main are still unchanged.

### 2026-11-27 matched source transition and reboot

Runtime source `0b5e8fa07e18be1c2dd3f607443309b0b2aa8efd` was built into one
hash-verified bundle: native assistant/Vault binaries and member/Chromium/Chrome
images. The installed two-member Ubuntu24 fixture moved from116a8cd to a different
member image. The first transition failed safely with the original container
retained stopped: a new registry lookup migrated schema10→11 while old connection
services still ran. The corrected order stops all registry consumers, installs
matching host code/configs, starts matching brokers, then replaces members and
resumes lifecycle/admission. Independent Claude Sonnet 5.5/medium reviewed this
ordering. The next run caught a harness seccomp path under `/root`, hidden by the
installed services' `ProtectHome`; runtime policy now lives under `/etc/moosuri`.
Neither failure changed the original member logical data snapshots or Vault.

The corrected [matched rollout harness](../../../deployment/kiri/linux-vm-smoke/guest-matched-rollout.py)
passed actual capsule startup/confinement/cleanup, both different-image member
replacements, UID/key/database/mount/allocation preservation and same-target
ID/PID replay. All six matching executables stayed stable before and after HTTPS
health. Lifecycle resumption retained both exact containers and member snapshots.
The original installer receipt, enrollment and encrypted Vault were retained.
Probe provenance is recorded separately; the bundle itself is not a signed release.

The real Linux Chromium acceptance then rejected a synthetic invalid model key
through the installed catalog endpoint, cleared its input, retained the unconfigured
state after reload and denied a second member the credential form. No inference
was called; this does not distinguish invalid-key rejection from provider outage.
A harness selector was corrected because Chromium's implicit select label included
its option text. An actual reboot was measured against the immediately preceding
boot ID. Both member identities, separate fresh Kiri recipients, cancellation,
logout, Vault handoff and admin boundary passed after reboot. Both existing Vaults
reopened their respective encrypted items at1280px/390px; wrong passwords and
cross-member visibility were refused. Screenshots were inspected.

Visual inspection also found bare controls in the new Claude setup form. It now
uses the existing management field/button styles and an explicit select label.
Exact current-source Linux engine tests17, typecheck/build and plan/link verification
passed. Candidate assets against the actual installed backend passed1280px/390px
field/default/touch-size/overflow checks and were visually inspected; no provider
call was made. This follow-up is not part of the0b5e8fa runtime acceptance above. Final-source
installation, production transition, actual managed OAuth setup and two-real-member
consent/conversation remain open. The complete0b5e8fa bundle is staged and hash
verified on the Linux host; no production runtime was changed in this work unit.

The reviewed candidate source and this evidence are being published as a development
preview on main. This is source publication, not a claim that production, the default
installer or all50 journeys are complete. The production server remains on its prior
runtime until the separately verified data-preserving transition is applied.


### Managed model recovery from an unavailable subscription

Production preparation found that the preserved model config uses a missing
owner-bound Claude subscription worker. Even with the managed setup endpoint,
the UI's API-only gate hid the existing explicit replacement path. It now exposes
that path for managed Claude subscription connections too. It names the switch
to the administrator's separately billed API connection before any submission.
The displayed subscription revision must match the managed file; missing or
mismatched setup refuses the form. No connection or credential is changed by
opening the screen. Independent Claude Sonnet 5.5/medium review prompted the
billing copy and revision check. Linux current-source engine cases20 and typecheck
passed. The actual replacement and inference still require an authorized API
credential and remain open; no developer subscription authentication is reused.

### 2026-11-27 production transition and actual fresh-connection defect

Public c8cf8e8 was deployed as one matched bundle on the existing Ubuntu 22.04 host
with Kiri 0.2.2. Member identity, allocation, mounts, logical data, encrypted Vault,
protected keys, installation roles/model bindings and existing Google grants were
preserved. Same-target member replay kept its container. Six services ran matching
binaries and certificate-verified app/about/Vault HTTPS passed. An immediate
Type=simple PID before exec caused one readiness failure; only matching executable
samples now count toward the stable interval. The transition resumed after brokers
without repeating backup, engine upgrade or member allocation. No installer receipt
was fabricated and the host OS/kernel were unchanged.

Post-transition Kiri UI opened the managed API recovery form without submission.
Existing Gmail read returned 200/completed/five metadata rows, which survived reload
in the original conversation. Wrong Vault password was refused; the correct password
opened the retained Vault. No mail text, token or secret entered this record.

The new portable Drive flow reached its fresh Vault recipient, then failed. Actual
proxy inspection showed DNS working but no external default route; Google CONNECT
returned 502. Kiri selects the default route from the primary network: creating
the proxy on public first, attaching internal with the exact private IP, then
starting fixes this. The installer now refuses old internal-primary or extra-network
proxies and checks the kernel default route. Interrupted create/attach/start and
actual command ordering are covered. The matched rollout harness uses the same order.
[Six real Linux egress checks](../../../deployment/kiri/linux-vm-smoke/guest-proxy-egress.py)
passed public Google HTTPS, private-address refusal, private-worker isolation, restart
and exact owned cleanup. [Production result and remaining gates](../../../docs/reviews/2026-11-27-matched-production.md).

The production egress repair prepared two new isolated proxy projects, verified
actual Google 302/private 403 and restart, and switched config plus matching smoke
evidence. Member identity/PID/mounts, keys and client were unchanged; matching
services stayed stable and real HTTPS passed. Original proxies remain for explicit
rollback. A subsequent real Vault-backed attempt reached opening but did not finish
authentication. The phone's Linux confirmation was expired, not a successful approval.
Explicit app cancellation cleared the socket, authentication service and browser
capsule. This live failure remains open for bounded page-state diagnosis.

Production invitation timer now runs the published worker with the existing profile,
a private three-UID policy and ten-GiB new-member capacity. Empty queue passes and
configuration preservation were checked; no second production user was created.
Next units: diagnose the opening/verification state with secret-free evidence;
final public installer/fresh-source acceptance; managed OAuth publication/recovery;
real second-member consent; valid administrator model connection and C01.

### 2026-11-27 live identifier-page correction

The post-transition opening stall was reproduced once with the retained Vault.
A read-only private observation in the one owned auth capsule returned only fixed
states/counts: official accounts origin, complete identifier document, visible
`input#identifierId[type=text]` and Next, no email/password-type field, and the
old classifier's `unknown`. No email had been submitted; the phone's expired
message was unrelated evidence and was not approved. App cancellation again
removed the auth capsule and reduced the socket connection count to zero.

The fixed classifier recognizes that exact Google text identifier in addition to
email inputs; unrelated text fields remain excluded. The existing TLS fixture now
uses the observed markup. Fifteen Linux DOM cases cover both variants, unrelated/
hidden/disabled/read-only/ambiguous fields, unavailable Next, exact origin and
password account binding. They and TypeScript checking passed. Seven Rust private
protocol/envelope cases passed. Independent official Claude Sonnet 5.5/medium
review found no material defect; `Chrome::identifier`/`password` already require
the `submitted` result before the one-shot state advances. Ten existing Kiri/official Chrome scenarios (GR-01–10) passed with the changed
text-field fixture: sealed delivery, same-profile consent, device wait, identity
and browser rejection, wrong allocation, stale release, sibling cancellation,
EOF/SIGTERM, denied/wrong-state callbacks and exact effects/cleanup. This is
synthetic-provider evidence, not a real Google login.

The new auth image retains the pinned production Chrome/SDK/proxy base and changes
only its Rust browser worker; the wire protocol is unchanged. The runtime switch
will retain existing gateway/member executables. Independent operator review
prompted durable atomic config/receipt writes, staged-file hashes, old-proxy health,
interruption handling and bounded draining. Preparation is separately probed before
any live configuration change. Final matched-source installation remains open.

Preparation and the narrow Google auth image switch completed. The new worker
image is `022c3980a991024012f3c4857df7fbbe31f0a17eca2d30e7de4c9a2f02b9692b`;
its installed config and evidence agree. Old owned proxy remains for rollback.
Actual UI/Vault attempts now leave the identifier page; fixed high-frequency
read-only observation saw official accounts origin and `rejected`, not mismatch,
before any password field. No verified login or consent occurred. Stop provider
retries; do not bypass its rejection with another browser or borrowed session.
Socket zero/only proxy and released phone reservation confirm cleanup.
[Installed result and resumption condition](../../../docs/reviews/2026-11-27-matched-production.md#live-identifier-correction-and-provider-rejection).


### Web invitation completion and pristine public source — 2026-11-27

I04 had an actual usability gap: the installed timer stopped at password_required
and the prior two-member harness finished with root CLI. That receipt never
proved invited-member web joining. Eight explicit J01–J08 branches now cover
completion, expiry/rotation, lost replies, restart, revocation, capacity,
concurrency and browser privacy. This is one cohesive admission unit, not eight
separate product goals.

Candidate code reuses the existing registry, pinned worker policy, UID claims,
private-volume initialization and member UI. Admin copies a 24-hour link;
invitee opens /join and chooses a password. Only the random capability digest is
stored. It binds issuer role revision and exact invitation allocation; ordinary
issuer logout does not revoke a delivered invitation, while role/suspension/
generation changes do. Password and capability stay in request memory/private
child stdin, never durable job records. One-time private initialization decides
replay. A link-issued invitation cannot fall back to timer/CLI activation even
after expiry or a lost response. Final activation rechecks the capability.
No invitation email is sent.

A separate untouched Ubuntu24 VM completed public HTTPS clone f0812c7a source
installation. Its baseline recorded no Kiri/Docker/Node/Rust/GWS/gcloud or Moosuri
state. Durable acceptance verified bundle provenance, both installed users and
encrypted Vaults in actual Chromium, real reboot and six egress/isolation checks.
All passed. It used the prior CLI credential step and excluded real Google/model.
That run is a prerequisite baseline, not acceptance of this new candidate.
The narrow production auth image and existing member data remain unchanged.

- [x] Complete integrated invitation store/HTTP/UI checks and independent Claude review.
- [x] Execute J01–J08 with actual installed Linux services/browser and preserve failures.
- [x] Publish the accepted admission unit and validate its matching bundle/four-member reboot.
- [x] Deploy the accepted admission unit with compatible registry consumers; dee84754 production preservation and post-transition GWS read passed.
- [ ] Reconcile pristine result and resume remaining Google/model/default-installer gates.


Independent official Claude Sonnet5.5/medium review completed in 37 turns.
Its capacity-starvation finding was not adopted: `worker::run` explicitly wraps
an empty page to cursor zero (`page.last().map_or(0, ...)`), and the existing
actual capacity harness retries two scans. The new installed J06 still verifies
capacity recovery rather than relying on that static conclusion.
The blocking-initializer finding was adopted: web completion and registry I/O
now run off Tokio async workers; status bypasses the initializer permit; the
private status child runs before the registry lock, while the one-time private
write remains capability-guarded. The critical child has a 15-second bound, to
be measured on the installed guest. Parent inspection also moved expiry-clock
sampling after registry-lock admission, with a contended-expiry regression.
Candidate checks passed before those targeted corrections: store51/52 (the sole
failure was a stale schema11 test expectation, corrected and passed), new link9,
member-chosen password1, gateway3, existing worker3, installer15 and frontend16.
The corrected paths passed Linux link-store10, gateway4 and the host build.
A second independent official Claude Sonnet5.5/medium review (14 turns) found
no material defect. Low-priority blocking-pool contention under distributed
invalid-token load remains a hardening item; expiry after private password save
requires a new admin link, as documented. No activation grace weakens expiry.
These results do not upgrade installed-journey or production status.

After the pristine baseline completed, its disposable host registry consumers
were switched together to the admission candidate (binary SHA256
b1c8a28dee1dc8c57b8efbf68f7daeba57d84f80d81a06edf1c84c61ec96d4bb), preserving
both original members. Member/auth images remain f0812c7a for this narrow
integration test; this is not a final matched release or a production switch.

Installed candidate J01–J08 branches and actual reboot passed, including two
web-created members with distinct Vaults. The capacity harness initially counted
occupied UIDs as free; its synthetic UID budget was corrected without replacing
users. Replacement-password denial was tightened and observed as HTTP401 after
reboot. [Exact evidence and limits](../../../docs/reviews/2026-11-27-web-invitation.md).
The future clean-installed harness now completes through the invitation API;
that revised full-install sequence is pending the final matched-source run.
### Linux public entry and interactive setup — next usable unit

The web-admission unit is public at9a7b83b3; its matching source bundle is building
in the marked disposable Ubuntu VM. Production remains on its recorded bundle.
Independent official Claude Sonnet5.5/medium source exploration (15 turns)
identified the README's older default path and missing public-input prompts.
The selected follow-up keeps the existing source entry and Rust setup, adds
interactive inputs and labels platform routes clearly. A signed one-command
Kiri release is separate packaging; it is not needed to meet the stated
public-source installation objective. Real Google/model acceptance remains open.

- [x] Validate public inputs/terms, non-terminal and password-pipe boundaries.
- [x] Exercise actual Linux terminal rejection/recovery and retain earlier install contracts.
- [x] Independent review and public entry/docs alignment.
- [x] Final matched fresh-source acceptance including interactive public entry; dee84754 resumed retained state after host-storage failure, then web admission/Vault/reboot/egress passed (supplied synthetic TLS).

Linux installer21 tests and the native build passed. The marker-guarded
[process harness](../../../deployment/kiri/linux-vm-smoke/setup-inputs.py) ran
the actual candidate under a PTY: invalid/same origins and blank certificate
terms refused before product writes; explicit acceptance reached only the
intentionally missing bundle. A nonterminal missing-input invocation left the
synthetic password pipe byte-for-byte unread. Source-entry missing/partial/empty
flags failed before downloads; both complete TLS modes and spaced/equals flags
passed the shell preflight. Existing product configuration/key hashes remained
unchanged. These input checks do not establish certificate issuance or I08.

Independent official Claude Sonnet5.5/medium review completed in19 turns.
Its late scripted-input finding prompted early shell presence checks, verified
with the process harness. Origin prompts now explain the accepted format;
recovery docs identify the receipt and same-answer/supplied-TLS requirements.
Its pristine-evidence objection cited the older interrupted116a8cd run: the
separate untouched f0812c7a baseline is valid and the guide now names and links
that exact evidence. No password or terms regression was found. The parent
inspected and tested the follow-up shell changes; no second review is claimed.

Final plan/link verification passed1,769 links and strict change validation
passed on Linux. Shell/Python syntax and diff whitespace checks passed. The
changed-file secret scan found only the pre-existing unusable `AAAA` PEM marker
in the installer tests; no new credential or exception was added. Dependencies
and third-party assets are unchanged. The candidate remains separate from the
running production bundle and the final fresh-source acceptance.

### Matching admission release and latest entry acceptance — 2026-11-27

The public9a7b83b3 bundle completed; all artifact/image hashes and both actual
confined browser admission/cleanup probes passed. All four existing synthetic
members moved to the new member image with original UID/mount/data/key identities.
Same-target replay kept ID/PID. Browser/Vault checks passed before and after an
actual reboot; both web-created users retained original passwords, replacement
HTTP401 denial, separate Vaults and member roles. The original installer receipt
and prior baseline evidence were retained. [Exact result](../../../docs/reviews/2026-11-27-web-invitation.md#matching-bundle-and-four-member-reboot).

A separate untouched Ubuntu24 VM is installing publicdee84754 through a real
controlling terminal: public addresses, administrator identity and hidden password
confirmation are entered at the product prompts. Supplied synthetic TLS avoids
external DNS/ACME; no real issuance is claimed. The durable next stages use the
current public invitation API harness for the second password, then both UIs,
encrypted Vaults, actual reboot and egress/isolation. No provider credentials exist
in that fixture. This latest-source run is still in progress.

Production is unchanged. A current-state operator preserves the existing Kiri0.2.2
engine, model setup, onboarding policy, account and grants while moving all registry
consumers together. Read-only checks confirmed its current paths and retired
transport fence. Independent official Claude Sonnet5.5/medium review prompted
earlier private-file/state preconditions, full command-reference checks, preservation
of browser/security settings, and identity comparison before public admission.
The first tool-based review could not read the operator and did not count as its
review; a supplied-source review and corrected closure review did. The closure
found no material defect in scope. Actual preparation/deployment waits for the
latest-source acceptance and available disk; no production upgrade was attempted.


### Latest public entry and production admission completed — 2026-11-27

The initially empty dee84754 guest exhausted host storage during installation.
The original failed evidence and incomplete receipt were preserved. After host
storage recovery, the exact complete bundle resumed through the real terminal
prompts and retained all receipt fields except completion. Durable acceptance
passed both members through public invitation API credential completion, actual
browser/Vault use, changed-boot-ID reboot and six egress/isolation checks. No
real provider or ACME credential was used.

The accepted bundle was hash-verified on production. Both confined browser
probes passed on its actual kernel. The already reviewed current-state operator
then preserved member/Vault/keys/registry identities/grants and switched all
registry consumers, member and browser images together. Exit0, matching stable
executables, SQLite integrity, original onboarding policy/timer and HTTPS200
were verified. Kiri page reload retained the session and Gmail account; GWS
returned five metadata rows, foreign member and invalid invitation were401, and
admin setup/invitation controls remained available. No production invite was
created and no model inference or new Google login was attempted.

Independent official Claude Sonnet5.5/medium read-only source trace (25 turns)
confirmed that the existing non-root worker/terminal login has no Kiri product
provisioning path: API setup cannot create the worker account/profile/unit or
owner binding. The trace is a bounded implementation investigation, not a fix
or real authentication. Pending model-connection choice is not inferred; the
reviewer's authentication is not copied. Fresh Google login remains blocked by
the already observed provider rejection. Continue independent implementation
without repeating unchanged provider attempts.

[Full latest evidence](../../../docs/reviews/2026-11-27-final-entry-production.md).


### Owner subscription prerequisite and private login bridge — 2026-11-27

The credential-free disposable Linux guest verified rootless systemd/cgroup 2,
actual container limits/isolation, the unchanged official CLI artifact and the
existing worker's logged-out result. Main integrated the official Claude-written
private child bridge, fixed blocking standard-input shutdown, reused worker
configuration/account/all-model validation, and wired the hidden CLI. Independent
Claude review prompted cancellable preflight and zeroizing deserialization;
closure found no material defect. The existing serde feature is now explicit.

Final Linux 57 Rust checks (15 engine, 42 connector) and 12 actual-process cases
passed. The official confined CLI emitted its authorization URL and cancelled
with actual container cleanup and no code, credential, browser login, inference
or binding. This does not complete M05/M06/C01: automatic service/profile/storage
preparation, Kiri gateway/UI, fresh owner authentication, binding and reboot
remain next. No production model choice, account or configuration was changed.
Eight supporting M05/M06 checkpoints keep those gaps visible without claiming
additional completed end-to-end journeys.

[Exact scope, earlier failures and reproduction](../../../docs/reviews/2026-11-27-claude-subscription-bridge.md).


### Credential-free host preparation — 2026-11-27

The root helper now prepares a dedicated service, verified artifacts and bounded
auth storage, reusing the installer and member-filesystem implementations.
Native launch admission remains separate from account authentication and root
binding authority. Ten preparation checkpoints expand M05-A/M06; the first
real run exposed an overlong Unix-socket path and retained that failure.
[Implementation, exact results and open gates](../../../docs/reviews/2026-11-27-claude-runtime-preparation.md).

P01–P09 actual Linux preparation/replay/quota/two-owner/reboot checks passed;
P10 final-candidate host-inode-default and reserved-login regression also passed;
P04's original socket-path failure is preserved. Final63 targeted Rust checks
and native build passed. Independent Claude initial/closure and focused inode/
reserved-UUID review completed. This is credential-free preparation only.
Root-bound evidence, owner-wide admission/login serialization, engine-home
resource limits, interrupted storage-record repair, Ubuntu22 portability and
existing gateway/UI/worker registration are explicit next integration gates.
No real login, inference, production configuration or model choice changed.

### Kiri subscription product integration — in progress

The prior turn was progress:51db7232 published credential-free preparation with
actual Linux evidence. This unit connects that prerequisite to the existing
administrator UI, private login bridge and owner-bound runtime-model worker.
Parallel official Claude Sonnet5.5/medium workers own isolated host-parent and
gateway/UI source copies; main owns integration, root/storage corrections and
Linux acceptance. No production setting or actual provider credential changes.

M05-A–H remain the acceptance scope: start on an empty server, official login
and model/account verification, reopen, cancel/disconnect, duplicate/out-of-order
code, wrong account/model, lost result, and authority/profile/reboot changes.
Preparation evidence alone still does not close these journeys. The initial UI
can offer the official single-use login link/code; remote-client rendering in
the same Kiri and ordinary member enrollment must be verified before claiming
the complete goal.

Candidate main changes bound the entire rootless worker home, reserve setup
capacity, reuse the OS subordinate-ID allocator on older useradd, recover
interrupted storage records and make root-generated configs immutable to the
worker. These are unverified until the integrated Linux results below.

Integration acceptance is now defined as S01–S07 real installed HTTP journeys and
U01–U03 real browser journeys. H01–H06 supporting Linux boundaries passed; the
H04 fixture-mode failure and correction are retained. Root and gateway patches
are integrated, but current source tests, installed acceptance and independent
closure are still in progress. Published/production state has not advanced.
[Product integration worklog](../../../docs/reviews/2026-11-27-claude-product-connection.md)
records the exact open lifecycle and provider gates rather than hiding them in
preparation counts.


### Installed connection checkpoint — 2026-11-27 12:56 KST

S01–S07 and actual Chromium U01–U03 passed on candidate7d3a9862, including
official URL/cancel, same-session reload, foreign session refusal, restart,
prepare cancellation and logout cleanup. Actual systemd255 exposed useradd
writes remaining read-only under full+/etc; the bounded allocator now uses
strict+/etc and the installed product passed. Initial failures are preserved.

Final source follow-ups guard terminal receipt monotonicity and prevent API
replacement from abandoning a published subscription worker. Explicit lifecycle
LC01–LC08 is next: disconnect/reconnect, API replacement cleanup, crash/reboot,
stale owner reclamation and member isolation. These are not marked implemented.
Real provider authentication, same-Kiri authentication rendering, ordinary member
enrollment, Gmail conversation and final matched deployment remain open.
No publication or production transition has occurred for this unit.


### Explicit owner disconnect/reconnect — 2026-11-27

The preceding installed-connection unit was published as b9596bb0 after final
U01–U03 and independent closure; production remained dee84754. This unit adds
explicit disconnect, exact target/operation separation, durable intent and root
retirement, v2 receipt compatibility, and return to fresh connection choices.

D01–D03 passed on the first lifecycle candidate. D04 reproduced a corrupt config
hiding the stored operation behind503; the correction was independently reviewed
and the retained failed fixture passed D04–D06. UI26 and model-setup47 checks
passed after the borrowed-test-fixture compile correction. Installed inspection
then found a stale public engine catalog after disconnect; exact catalog
withdrawal and late-publication fencing passed the focused store journey and
independent review. Final4aaa3225 passed actual Linux D01–D06 and Chromium
DU01–DU03, including catalog withdrawal across reload. All prior failures and
source-specific results are retained in the
[unit worklog](../../../docs/reviews/2026-11-27-claude-disconnect.md).

LC03 automatic API-swap, LC05 authenticated-worker reboot, LC06 generation-wide
reclaim and LC07 member enrollment remain open. Preparation and synthetic Active
fixtures do not close real login, model inference, Google/OAuth or Gmail goals.

### Same-Kiri Claude authentication — verified partial unit

- [x] Integrate the private headful browser, session-bound frame/input and automatic official callback delivery into the existing subscription flow.
- [x] Verify installed screen/session/stale-input/cancel/restart/logout and synthetic callback boundaries on Linux; resolve material independent review findings and document each scope limit.
- [ ] Verify the real provider callback/SSO and K06 with the owner's fresh authentication; do not borrow reviewer credentials or count synthetic callbacks as provider acceptance.

[Six supporting journeys and current evidence](../../../docs/reviews/2026-11-27-claude-kiri-auth.md)
expand the existing 61 journeys without replacing their full installation/Google/
model/Gmail acceptance gates. Installed K01/K02/K03/K05a–c passed, and actual
Chromium U01–U03 passed again after correcting the unreadable mobile frame with
expand/pan controls. Native browser166/model-setup53/render3 and UI25 passed;
independent21/39/17-turn official Claude reviews closed the material source
findings. Real account/inference and a final-source empty install remain open.
Production stays dee84754; this unit is a tested source candidate.

### Installed Vault preflight — 2026-11-27

GV01–GV06 passed across preserved initial and bounded resume runs: empty/one/two
Google account selection, different identity, encrypted save, exact ciphertext
retry, foreign-member non-disclosure and popup/logout cleanup. Wrong status
expectations in the harness were corrected after observing real adapter behavior;
no product auth change was necessary. Final GV05/GV06 added positive key inventory,
unknown-challenge and explicit final cleanup controls and passed. Independent
official Claude Sonnet5.5/medium review completed. Earlier checks' evidence limits
and failures remain in the [unit report](../../../docs/reviews/2026-11-27-google-vault-preflight.md).
No actual Google login/consent/inference or production change occurred.

### Pending Google connection reentry — candidate verified

R01 exposed a real installed UI defect: reload forgets the prepared job, and the
next Connect conflicts with its own remaining key. The six RE01–RE06 supporting
journeys, correction contract and actual/synthetic evidence distinction are in
[the reentry worklog](../../../docs/reviews/2026-11-27-google-reentry.md).
Use the existing server-owned job and same-login metadata; do not start another
authentication or store credentials to recover.

Reentry implemented in the existing UI/status protocol. Linux focused64 checks,
web/typecheck/native build, actual installed RE01–RE05 and explicit cleanup passed.
Official Claude13/5-turn reviews resolved completion-loss and cancel-message
findings; real provider paths stay open. A distinct first-thread mailbox race was
reproduced on unchanged public abc37d1c; the next entry records its correction.

### First-conversation mail request — verified candidate

The selected thread reference now updates immediately with thread creation.
FM01–FM03 passed in actual installed Linux Chromium with explicitly synthetic
interpretation/Gmail responses; the original request runs once and a newer draft
survives reload/resume even when its text is identical. Focused79 UI checks and
web/native builds passed; independent official Claude Sonnet5.5/medium static
review found no material defect. [Evidence and limits](../../../docs/reviews/2026-11-27-first-conversation-mail.md).
No real provider acceptance or production deployment is claimed.

### Installed shared app distribution and recovery — 2026-11-27

GA01–GA05 passed on real installed Linux services; GA06 passed in a separate
resumed run after a transient-response harness bug. Shared app, private member
keys, idempotent save/conflict rejection and member/service recovery were verified.
Three actual Chromium checks preserve the distinction between installation
registration and each member consent. Independent official Claude review found
cross-run proof and report recovery gaps; the harness was corrected without
claiming stronger historical evidence. [Results and exact limits](../../../docs/reviews/2026-11-27-google-app-sharing.md).
Synthetic app only, no provider or production change. Final-source and real
Google/Claude/two-member acceptance remain open.

### Ubuntu22 Claude preparation compatibility — 2026-11-27

Actual stock Ubuntu22 preparation exposed omitted native subid defaults being
rejected. The fix accepts documented defaults and trusted empty databases while
refusing malformed/ambiguous settings before allocation. Fifteen focused Linux
tests and native build passed; independent official Claude7/6-turn review closed
the material findings. Six supporting journeys passed through12 native checks,
including two owners and an actual reboot. Four earlier product/prerequisite/
harness failures remain recorded. [Scope and evidence](../../../docs/reviews/2026-11-27-claude-ubuntu22.md).
No real authentication, inference, OS upgrade or production change occurred.

### Current-source installation acceptance — preparation

The next empty Ubuntu24 installation includes all published subscription,
Google reentry and first-mail changes. Inspection found the first-mail browser
dispatch passed two filenames to `Path.with_name`, raising `TypeError` before
FM01–FM03 could run. Corrected that invocation and exercised all seven sibling
module path expressions on Linux. This restores the runner; it does not add new
browser/provider acceptance to the earlier first-mail results. The current-source
empty installation, two members, real reboot and production transition remain
open until the recorded execution completes.

The administrator, engine and subscription guides now lead with the implemented
automatic preparation and same-Kiri login screen. Manual worker/JSON setup is
operator reference; real authentication/inference remains explicitly open.
Existing-host dependency guidance distinguishes Docker CE's rootless tools from
distribution packages to avoid replacing the engine during an additive repair.

### Current-source cancellation failure and correction — 2026-11-27

Fresh public46b76b44 installation, public second-member admission, actual
two-member Vaults, reboot and proxy confinement passed. Claude U02 cancellation
failed with cleanup pending; actual same-owner UI recovery passed separately.
FM01–FM03 also passed in a separate resumed browser run. The coordinator's
failure remains preserved and production transition still requires successful
final acceptance. A Linux delayed-output cancellation regression failed before the
fix and passed afterward with all11 focused connector tests. The actor now
drains private late output during its existing bounded shutdown; independent
official Sonnet5.5/medium review found no material regression. Corrected gateway
Chromium U01–U03 and six installed API checks passed, including cancel, restart
recovery and logout. This was an overlay on the retained installation; the final
corrected-source bundle and rollout remain pending.
[Evidence and remaining gates](../../../docs/reviews/2026-11-27-current-source-acceptance.md).
