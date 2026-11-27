# Current public source: empty Linux installation and cancellation failure

An empty Ubuntu24 guest cloned public revision `46b76b44`, built its host and
member/browser artifacts, and completed the actual terminal installer. Baseline
checks found no product/provider state or preinstalled Kiri, Docker, Node, Rust
or GWS. Supplied synthetic TLS was used; this does not prove ACME issuance.
The later `88b407bc` changes only connection documentation.

## Observed journeys

| Scope | Result |
| --- | --- |
| I01/I08 public source and terminal setup | Build and installation completed without a restart or developer credentials. App and separate Vault HTTPS were usable. |
| I04/J01 invited second member | Public web invitation API accepted the second password; distinct UID, volume, GWS and browser recipients. No root CLI password completion. |
| I05 replay/conflicting inputs | Same inputs retained identities and authority; changed origin refused without mutation. |
| I02 and member boundaries | Unconfigured model remained unavailable; only the administrator could open setup. Foreign recipients, stale handoff and logged-out access were refused. |
| V04 and two-member Vault support | Actual Chromium created both 8-character Vaults, saved encrypted synthetic items, refused wrong passwords and verified member separation. |
| I06 reboot | Actual changed boot ID; both identities and encrypted Vaults reopened; administrator/member boundaries retained. |
| Proxy confinement | Six actual checks passed, including public HTTPS, private destination refusal, no external worker route and retained egress after proxy restart. |
| Claude U01 | Mode choice, exact Sonnet5.5/medium defaults, hosting consent and mobile form passed. |
| Claude U02 | **Failed:** cancellation left `cleanup_pending` with browser intent retained. Docker inventory was empty, but that alone did not override the missing shutdown receipt. |
| Same-owner cleanup recovery | A separate actual browser run used **정리 다시 시도** and reached cancelled. Original failure evidence remains retained. |
| FM01–FM03 | A separately resumed installed Chromium run passed one first-message read, explicit reload resume and preservation of a newer different/identical draft. Interpretation and Gmail replies were synthetic; no model inference or provider authentication. |

The actual Claude screenshots show the provider's Cloudflare security verification
page inside Kiri. Desktop/mobile frame display, reload and expansion are visible;
this run does **not** establish arrival at the account form or authenticated login.
The original coordinator remains failed. Neither the successful recovery nor the
independent FM run converts it into a passed release proof.

## Cancellation correction

The host actor dropped its stdout reader before waiting for worker shutdown.
Cancellation can arrive while a frame is being written: dropping the pipe can
break the worker/Docker output, while keeping it open without consuming it can
block shutdown. A Linux regression using a delayed 256KiB response reproduced
`CleanupUnconfirmed` on the unchanged code.

The correction retains stdout from startup through cleanup and discards late
output privately in a fixed-size zeroizing buffer. The existing cleanup deadline,
required successful worker exit and exact container reconciliation remain in
force. Late frames/codes are not parsed, adopted or replayed. All11 focused
connector tests passed, including the previously failing regression and the
stalled-worker refusal. Independent official Claude Sonnet5.5/medium reviewed
the frozen change and found no material regression.

The corrected native gateway
`90951364617f90bb4db1610df8b71c9da698ab7e0620ea3cd21603cf17764ed6`
was then selected in the same disposable installation. Actual Chromium U01–U03
passed, including confirmed cancellation without a cleanup retry. The six
installed API checks K01/K02/K03/K05a/K05b/K05c also passed: private frame,
session/member boundaries, stale and duplicate gestures, explicit cancel,
gateway restart/recovery and logout. The provider screenshot still shows
Cloudflare verification; no account authentication or inference was performed.
This is a diagnostic host-gateway overlay, not a rebuilt installation bundle;
the original installer receipt remains `46b76b44`. An immediate post-restart
hash observation raced process execution; subsequent active/running state,
zero restarts and exact binary hash were verified before the UI/API checks.

The runner now reports failures at the Claude UI module's own line rather than
its dispatch call. Production remains `dee84754`; no reviewed `46b76b44` staging
or transition was executed. Final corrected-source installation, matched rollout,
real Google/Claude authentication, automatic OAuth setup and two real member
consents remain open.

## Google bootstrap cancellation and retained installation baseline

The same stdout lifetime defect existed in Google's actor. An actual isolated
Docker transport probe in the next empty-source guest produced exit1 when the
host closed the pipe before a tiny final reply, and exit0 when it consumed the
reply through shutdown. Both exact diagnostic containers were confirmed absent.
No Google account, grant or OAuth resource was used by this probe.

A deterministic Linux actor regression emits its final reply only after receiving
the host's shutdown command. It failed with `CleanupUnconfirmed` before the fix.
Google now retains and privately drains stdout during the existing cleanup grace;
late OAuth replies are never parsed, delivered or saved. The negative regression
still refuses a worker's failed exit. All44 capsule tests passed after the change.
Independent official Claude Sonnet5.5/medium review found no material actionable
defect in the frozen change. These support R02/R03 but do not prove remote SDK
grant revocation or an installed OAuth bootstrap cancellation.

The next guest originally cloned public `75c1b533` from an empty Ubuntu24 baseline.
Its source build was deliberately stopped after this defect was confirmed, before
any product installation. The unit was inactive/dead with no surviving Cargo,
Rust or BuildKit process; `/etc/moosuri`, `/var/lib/moosuri-control` and
`/srv/moosuri-storage` were absent. The original clean source, baseline and
interruption record are retained, and its original acceptance coordinator is
failed. Reusing this guest requires an explicit original-baseline-to-final-revision
recovery record and fresh proof that product/provider state is absent. It must
not be described as a newer revision's untouched initial baseline. Production
remains `dee84754`; the old revision's staging drafts are not release approval.

## Public2608 recovered source installation completed

The preserved pre-product `75c1b533` build baseline was explicitly recovered to
public `2608a12e`. The original failed coordinator and interruption remain in
place. The new coordinator completed source/terminal installation, second-member
public invitation credential completion, two actual encrypted Vault/browser
journeys, actual reboot, proxy confinement, Claude U01–U03 and FM01–FM03.
Its terminal result explicitly records `baseline_kind=pre_product_build_recovery`,
`original_revision=75c1b533`, `real_google_and_model=not_run` and
`real_acme=not_run`. The first-mail responses remain synthetic; U02 exercises an
unauthenticated provider screen and cancellation. No real account login occurred.

The accepted bundle's running gateway SHA-256 was
`d47ceb6972228525eb27fb2d7a516d6ba1591501498cbe2d34cc05818987623c`.
The acceptance result and separate original failure are retained in the Linux
fixture's `recovered2608-acceptance-progress.json` and `acceptance-progress.json`.
This closes the corrected-source installation run, not the provider or production
transition gates. A later gateway-only diagnostic checks
[existing-subscription recovery](2026-11-27-claude-legacy-recovery.md);
that overlay is not the original accepted bundle.

## Final public c3 installation — passed from an untouched guest

Public `c3d18bd96af1addef8afd4884a368e9b1d63007a` was cloned on a separate
empty Ubuntu24 guest. Its recorded baseline had no product state, provider
configuration, Docker/Kiri, Node/Rust, GWS or gcloud. The public source installer
built the complete bundle, accepted all six actual terminal inputs and completed
without a build recovery or diagnostic binary overlay. Only synthetic TLS was
supplied; real ACME issuance was not exercised.

The single coordinator finished with `stage=complete`, `status=passed`:

| Installed journey | Observed result |
| --- | --- |
| Administrator and separate Vault | HTTPS sign-in and single-use handoff; repeat setup preserves identity; changed origin refused. |
| Second member | Public web invitation password completion, separate UID/GWS/browser recipients, role and replay boundaries. |
| Both actual browser/Vault sessions | Eight-character Vault creation, recovery confirmation, encrypted save/reopen, wrong-password refusal and member isolation. |
| Actual guest reboot | Both identities, stored Vault items, services and administration boundaries survived; browser checks passed again. |
| Browser confinement | Public Google HTTPS, private-address refusal, isolated worker routing, proxy restart and owned cleanup passed. |
| Claude U01–U03 | Same-Kiri screen, mobile/reload, foreign-session refusal, confirmed cancellation and ordinary-member exclusion. |
| First mail FM01–FM03 | Original request runs once; reload needs explicit resume; a newer draft survives. Replies are synthetic. |
| Legacy connection LR01–LR06 | All six passed in one fresh run; durable cleanup intent survived reentry and synthetic fixtures were restored. |

The mobile Vault screenshot was inspected: controls and the saved synthetic item
fit the screen. FM and LR results both identify the same running gateway as the
bundle. Acceptance explicitly retains `real_google_and_model=not_run` and
`real_acme=not_run`; this does not establish account authentication, inference,
automatic OAuth issuance or two real Google consents.

The source manifest SHA-256 is
`bbe4189db1d2e5a60688896eb4da928647a90c932766013485ea83eb8c716271`.
Running gateway:
`08a8667f26435ee05f9694cec7b5bb523cb7d06d1cf479bae5657d074a50ce18`.
Running Vault:
`27cd5f3d77554a091162ed43e8484273f508dca653dc9f081d46655aea6256fe`.

## Matching c3 production transition

The concrete staging and transition operators received an independent official
Claude Sonnet5.5/medium read-only review; observed model matched the requested
model and no material defect was found. Their SHA-256 values are
`5d6036f03d5acc11b4f0fa476e4a8112f0f6af2e9eb45d05a0f2965cf15d4087` and
`4f2637d96c98fa58fccb678083ac001ef32f8e0372fe741b009a4ebe65440809`.
The staging gate required the exact successful coordinator, restored single-run
LR evidence and matching FM/LR/running binary hashes. Every bundle artifact was
copied and verified on Linux. Production transition and real provider results
are recorded separately below when observed.

The transition completed on the existing Ubuntu22/kernel5.15.0-179/Kiri0.2.2
host. It saved a consistent private backup and matched the gateway, member,
Vault and browser images to the accepted bundle. Member identities, mounts,
encrypted Vault/data, keys, allocation records and Google grants were preserved.
Service stability, effective commands, HTTPS and SQLite checks passed. Existing
onboarding policy/timer remained; no new-installer receipt or OS upgrade was made.
The original Kiri member session survived a reload and displayed the new setup.

### Legacy gateway egress and real provider follow-up

The first real Claude attempt failed during host preparation, before displaying
authentication. The fixed diagnostic was `claude_subscription_prepare_host_prepare_failed`;
the attempt retired and the previous model configuration remained intact. The
existing host still had a loopback-only gateway cgroup rule absent from the
public installer. These three supporting I07/M05 journeys record the correction:

| ID | Action and result |
| --- | --- |
| UP01 | A credential-free HEAD request to the pinned official CLI under the same legacy IP policy timed out with exit28; the public installer's policy returned HTTP200. |
| UP02 | An independently reviewed repair reset only the gateway's two obsolete IP filter settings. Its first post-check timed out; the failure is retained. A separate read-only 12-sample observation verified the stable exact binary, unchanged command, only the expected policy delta, unchanged protected files and healthy HTTPS. Volatile `ExecStart` timestamps/PID were excluded from command equality. |
| UP03 | The actual UI retry downloaded the exact 341,914,128-byte official CLI, completed private runtime preparation and displayed the official Claude login screen inside Kiri. It did not replace the previous connection before authentication. |

The repair's SHA-256 is
`0de2c233f08e49fd537746f32dee46f3015f1f78fce7a68131dbe0e1f4205351`.
Official Claude Sonnet5.5/medium reviewed the frozen repair with no material
finding. This changes the gateway's outbound policy to the published installer
defaults; it does not change the host firewall or member/browser confinement.
The repair timeout and subsequent `verification-after-timeout.json` are separate
private evidence. The original observer's timeout cause is not asserted.

Actual post-transition GWS returned HTTP200/completed with five inbox metadata
items and matching thread/request/mailbox. Its saved history returned five items
without a second provider read. A different asserted member was refused with401.
No mail contents or provider tokens were copied to these records. This proves
existing grant preservation, not natural-language model interpretation.

The live Claude screen showed Google, Apple and email choices. After the
**Continue with Google** action, the member view lost its frame, remained in
`awaiting_code`/screen-waiting and later failed with browser cleanup complete.
No Google identifier/password or Claude verification code was submitted. The
cause is not established; this is an open real SSO screen-transition failure,
not evidence of provider rejection or successful authentication. K06, real model
inference/Gmail conversation, fresh automatic Google/OAuth setup and two real
members' consents remain open.


## Integrated public e503 installation — passed

A new empty Ubuntu24 guest cloned public
`e5038af811c9f2aa609db9ee6aa717c2ab6faa35`, including the popup, diagnostics,
authentication memory/PID limits and first-Google-setup UI changes. No product,
provider configuration or Docker/Kiri/Node/Rust/GWS was preinstalled. The actual
terminal source installation completed without a build recovery or binary
overlay. Its single coordinator finished with `stage=complete`, `status=passed`.

Both members completed public invitation/password admission and actual browser
Vault journeys: eight-character passwords, encrypted save/reopen, wrong-password
refusal and member isolation. An actual reboot preserved both identities, stored
Vault items, role boundaries and routing. Six network/cleanup checks, Claude
U01–U03, first-mail FM01–FM03 and existing-connection LR01–LR06 all passed on the
same installed bundle; the temporary legacy fixtures were restored.

TLS and the mail/model responses were synthetic. The explicit results remain
`real_acme=not_run` and `real_google_and_model=not_run`. This does not complete
new OAuth creation, real provider authentication, natural-language Gmail or two
real member consents. Earlier failed coordinators and their evidence are retained.

The exact installed and staged manifest SHA-256 is
`6d154c9516353d3482dbeb37d7e180fb40f4f4fe43c19a216e5d17a05302c569`.
Gateway SHA-256:
`b1ec271f5fa13ce7660d0a694ac233ad5606f84e6ccb3c192ad347125eacd5b5`.
Vault SHA-256:
`a115be93af7b08463d34908f809138678f9b1ba2e13ad2f264ac0ddea1a53989`.
The root staging proof binds the two members, changed boot ID, successful
coordinator, restored LR fixtures and matching FM/LR/running gateway to this
manifest. Its SHA-256 is
`6c57891921f32149ddd605ffd096ded813ae2819a547dd0ff06cfc88b60906cf`.


### Matching e503 production transition and real UI follow-up

The same complete bundle replaced the c3 member/Vault/ordinary-browser base and
e29 gateway/auth patch on existing Ubuntu22/kernel5.15/Kiri0.2.2. Both new host
binaries executed on Ubuntu22 before transition. A consistent private backup was
saved; old releases/images/proxies remain available. Exact member/Vault logical
contents, ownership/modes, protected keys, allocation/registry identities, model
configuration and member/service confinement were preserved. Database checks
used stable private copies including WAL, avoiding root-created SQLite sidecars
in live member storage. All eight effective service commands point to e503.
The original onboarding policy/timer remains; no installer receipt was adopted.

Independent official Claude Sonnet5.5/medium reviewed the frozen operators and
corrections. Optimization now refuses execution before any mutation; exact
manifest bytes and current command/configuration pins are bound. A real Linux
transient service restart verified that normalized command comparisons ignore
volatile PID/timestamps while retaining static fields. The same c3/e503 registry
and rollout source plus the earlier successful equality check resolved the
review's migration concern. Actual production command/inventory and private-WAL
copy checks passed. Final staging/operator SHA-256 values:
`8e7f75d68947a5baf2d075c1be6b7eb1fc1f738142bacd8df7ea085fe1d0a032` /
`f777d404ab79c300c080723e921266c8227d15efc27708e7c87b6a1456552d9d`.
Only artifact pins and the opening description changed after the reviewed logic;
an AST comparison confirmed that boundary. Public Google HTTPS probes and
private-address refusal passed; no credentials or OAuth resources were submitted.

The existing authenticated Kiri session and Google connection ID/epoch/mailbox
survived. One actual GWS request returned HTTP200/completed with five metadata
items, correct thread/request identity and exactly one saved receipt. A foreign
asserted member received401. Reload displayed three saved receipts of five rows
each and made zero inbox requests. No raw mail or tokens were exported.

The actual composer then submitted “최근 받은 메일 5개 확인해줘”. Work interpretation
returned422 and visibly reported that the assistant model could not connect.
The draft remained; there were zero inbox requests and no additional receipt.
This is an observed remaining authentication/model gate, not a completed
natural-language Gmail journey. The provider security check needs resolution
before real Claude inference; no fallback or borrowed authentication was used.


### C04 real response loss and recovery without provider access

A later real request used the same deployed e503 member and existing Google
grant. Kiri CDP intercepted only that inbox endpoint at the response stage and
observed HTTP200, then failed delivery with `ConnectionClosed` before the
browser received the body. No response body or mail contents were exported.
The normal history API already contained exactly one completed five-item receipt.

The sole installation GWS relay was stopped temporarily with a root restore timer
armed. The member remained running with `NetworkMode=none`; the gateway and
lifecycle stayed active. Replaying the exact thread/request/connection/epoch
returned HTTP200 and the identical complete receipt while provider transport was
unavailable. History still contained that request once, proving recovery did not
need another provider read. The GWS relay was immediately restored with the same
binary; member identity/image and protected keys/configuration were unchanged.
The restore timer was then stopped.

Actual Kiri reload displayed four saved receipts with five rows each, made zero
inbox or work-interpret requests and retained the prior model-unavailable draft.
Temporary test state was cleared. Sanitized evidence is retained separately from
the original deployment receipt. This closes the supported read API's real
response-loss/replay boundary; it does not claim natural-language initiation or
new Google consent, which remain C01 and the provider journeys.
