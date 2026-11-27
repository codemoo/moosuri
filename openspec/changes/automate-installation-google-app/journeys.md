# Google connection user journeys

Acceptance starts with an unrelated person cloning the public project onto an
empty Ubuntu 24.04 server. The existing owner's live installation is a separate
provider experiment. A component test or operator-assisted Console setup does
not close the portable end-to-end gate. Use synthetic identities for destructive,
cancellation and cross-member scenarios; authorized real accounts for Google
consent and read acceptance. Never retain raw mail, secrets or provider traces.

Status: **live pass** means observed on the existing Linux installation;
**diagnostic pass** means the corrected disposable installation described in
[tasks](tasks.md); **open** requires implementation or the stated full journey.
Prior component evidence remains in the task log and does not upgrade an open
journey. Latest matched public-source installation and production evidence is in the
[final entry report](../../../docs/reviews/2026-11-27-final-entry-production.md). All model execution and independent review uses Claude Sonnet 5.5 / medium.

## Install and first use — 10 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| I01 | Empty server; obtain public source and follow the default installer | HTTPS app and separate Vault work without private helpers, preinstalled Kiri/GWS or developer credentials | Public dee84754 empty-start source/terminal installation passed after host-storage interruption recovery using the same bundle and receipt; the second member completed public web admission; both members passed actual browser/Vault and reboot acceptance. Supplied synthetic TLS; real ACME/provider setup not tested |
| I02 | First admin opens the installation without a model connection | Clear admin setup; no provider calls, guessed model or local fallback | Candidate116a8cd installed service/setup and two-member denial passed, including actual reboot; real configured model is I03/M01 |
| I03 | Admin attaches an authorized model connection and selects Sonnet 5.5 / medium | First real chat and work interpretation use that selection; credentials stay private | Open: first-provider setup and current interpretation failure |
| I04 | Invite a second member after all services are already running | Independent UID/volume, browser/Vault/GWS and model sockets appear without manual service editing | Public dee84754 new installation completed the second password through the web invitation API and retained distinct UID/volume/recipients across actual reboot. J01–J08 installed fault journeys also passed; admission is deployed, but a real second production member is still open |
| I05 | Stop installation after preparation, then run the same installer | Original installation, member allocation and keys retained; conflicting inputs refused | Candidate116a8cd recovered prior build interruption; complete same-input replay and conflicting-origin refusal preserved identities/keys |
| I06 | Reboot a completed installation with two members | Same identities and connections, usable app/Vault and services; no duplicated requests | Candidate116a8cd/0b5e8fa and matched public9a7b83b3 actual reboots passed; latest admission run retained four members, independent Kiri recipients, handoff, roles and encrypted Vaults/items. Latest interactive-source dee84754 two-member installation and actual reboot also passed after storage recovery; Google/model connections still unconfigured |
| I07 | Upgrade an existing Kiri installation with an encrypted Vault and connected member | Preserve member/volume/key identities and grants; validate host/engine compatibility before a matched component switch; no unrelated host OS change | Same-kernel Ubuntu 22.04/5.15.0-179/Kiri 0.2.2 member recovery, both confined browsers and six owned profile-rollout checks passed; different-image candidate0b5e8fa transition and real reboot passed after correcting shared-service order and the confined seccomp path; production c8cf8e8 and latest matched dee84754 transitions retained the real account/Vault/keys/grants and post-transition existing Gmail read passed; fresh login exposed missing proxy egress, repaired with real public/private and restart checks; post-repair authentication remains open; no new-installer receipt is fabricated |
| I08 | Follow README's Linux route, run source setup with no flags from a terminal | Build the selected public revision; prompt for distinct HTTPS origins, certificate contact/terms and administrator; reach the member app | Public dee84754 actual terminal source install passed with supplied synthetic TLS after retained-state storage recovery; public origins/admin/hidden password prompts exercised. Real ACME issuance remains untested |
| I09 | Enter invalid/same origins or decline certificate terms | No product configuration or member created; existing prerequisite/build outputs are reusable | Actual Linux PTY invalid/same origins and blank terms refused; configuration/key hashes unchanged. Accepted terms reached bundle validation only; full install is recorded in I08 |
| I10 | Run unattended with complete flags/password pipe, or omit a required public input | Complete flags remain compatible; missing values fail without reading the password as an address or waiting for terminal input | Actual Linux password pipe remained unread; missing/partial/empty source-entry flags failed before downloads; complete supplied-certificate and Certbot preflight passed in both flag forms. Full installation is recorded in I08 |

## Invited member joins on the web — 8 journeys

These expand I04. The earlier installed test used root CLI for the second
member's password; that is partial infrastructure evidence, not web admission.

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| J01 | Admin reserves a member and copies a link; invitee opens it on mobile and chooses a password | First login needs no operator command or second password change; own Vault and connections appear | Installed mobile web joining, matched9a7b83b3 four-member rollout/reboot, and final dee84754 new-source web credential completion/two-member Vault/reboot passed; deployed to production without creating a new live invitee |
| J02 | Open malformed, expired or replaced link | No account/password write; clear request for a new link; replacement preserves allocation | Installed expired/replaced links passed; malformed cases passed store/HTTP/UI checks |
| J03 | Password submit succeeds but its reply is lost; check status and reopen link | Report already complete; original password works, a newly submitted password cannot replace it | Installed real successful redemption with lost reply, status recovery and original-password login passed |
| J04 | Restart after private password initialization but before route activation | Same UID/volume; valid link resumes; timer/legacy path cannot bypass link authorization | Installed gateway kill at initialized stage, timer fence, same-allocation resume and actual reboot passed |
| J05 | Issuer is demoted/suspended or invitee is suspended/replaced before completion | No later initialization/activation; another current admin may issue a new link for the same eligible reservation | Installed invitee suspend/resume/new-link passed; issuer demotion/replacement passed registry tests |
| J06 | Capacity is full, timer is busy or preparation is delayed | Explain pending state, retain original request/UID claim, discard submitted password and permit explicit retry | Installed full capacity, cleared password and same-reservation timer recovery passed after correcting fixture UID capacity |
| J07 | Double-submit, use a second tab, or replay completed link with a different password | One initializer and one account; no duplicate UID or password overwrite; second member stays isolated | Installed replay, original-password and replacement401, two distinct members passed; simultaneous-submit exclusion passed component tests |
| J08 | Reload join/admin pages or make cross-origin/member administration requests | Link/password absent from URL after admission and persistent browser storage; private POST only; admin boundary enforced | Installed fragment removal, storage/reload privacy, foreign-origin and member/admin denial passed |

## Model first use and recovery — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| M01 | Empty installation; admin registers the shared Anthropic API connection | Both first members use Sonnet 5.5/medium; API credential stays in the host's private setup directory | Linux synthetic setup/socket boundaries passed; real API credential and inference open |
| M02 | Invite another member during an existing member's model request | New socket appears; existing request is not cancelled or charged twice | Dynamic socket isolation passed; full install journey open |
| M03 | Wrong/revoked key, unsupported model or invalid effort | Show the actual unavailable state; preserve an existing connection until the replacement is valid | Installed candidate0b5e8fa Chromium UI rejected one synthetic invalid-key catalog request, cleared input, stayed unconfigured after reload and hid setup from a second member; valid replacement and inference remain open |
| M04 | Lose API-setup reply, then another administrator changes the connection | Reconcile this request's revision; never attribute another administrator's save to this request | Candidate UI attribution and atomic-save tests passed |
| M05 | Administrator elects an official Claude subscription connection | Provision isolated non-root worker, fresh login and exact member binding from public source; no copied developer authentication | Public b9596bb0 administrator UI/preparation/official-URL/reload/cancel/restart checks passed; explicit owner disconnect/reconnect and corrupt-config recovery are in Linux acceptance; real authentication, inference, same-Kiri authentication and member enrollment remain open |
| M06 | Worker is missing/restarted, member generation changes, or server reboots | No borrowed member login, no fallback model, no new charge before dispatch; truthful recovery | Socket isolation/replay boundaries passed; fresh subscription/reboot journey open |

Shared model credentials use the installation API route. The existing official
Claude subscription worker is pinned to one member and authenticated principal;
it cannot serve a second member by copying its binding or credentials. Source
inspection is not evidence of successful fresh provider login or inference.

### M05/M06 subscription connection checkpoints — 8 supporting journeys

These expand the existing model journeys, not the 61 end-to-end acceptance rows.
A passed child-process check does not complete its Kiri UI or provider journey.
The installed setup must still prepare the service and bind it to the current
member; the user must not have to create Linux users or private JSON by hand.

| ID | User action or interruption | Observable acceptance | Current scope |
| --- | --- | --- | --- |
| M05-A | Administrator chooses a fresh subscription on an empty server | Product prepares verified artifacts, bounded storage and a dedicated worker, then opens official authentication in that member's Kiri | Credential-free rootless/artifact/isolation feasibility passed; automatic credential-free preparation and actual reboot passed; authenticated gateway/UI integrated and actual installed preparation/URL/cancel passed; account authentication and binding remain open |
| M05-B | Owner completes official authentication | One original login process receives one code; account and every configured model/effort verify before readiness; binding then rechecks current owner/session/revision | Private child synthetic and real-process checks passed; real account authentication/binding open |
| M05-C | Owner reopens a setup with an already verified account | Verify the same account/catalog without a second login or inference; preserve the active binding | Synthetic catalog/skip-login check passed; installed UI recovery open |
| M05-D | Owner cancels, closes the page, or loses the connection while entering the code | Cancel and clean up the original process, including partial input; never save a ready binding | Synthetic and actual-binary cancellation/EOF checks passed; official CLI URL/cancel/cleanup passed; installed page URL/reload/cancel passed (U02) |
| M05-E | Owner double-submits, pastes a malformed code, or submits before the login step | Reject without retrying authentication or executing a second login | Synthetic malformed/order/duplicate checks passed; installed malformed-input/idempotence/session boundaries passed (S02/S03); actual credential submission remains open |
| M05-F | Owner signs into a different account, lacks a subscription, or loses a selected model/effort | Fixed unavailable status; no inference, fallback model or binding replacement | Synthetic identity/catalog checks passed; real provider cases open |
| M05-G | Login stalls, output disappears, or its result is lost | Bounded cleanup; do not claim ready from a missing result or replay a one-time code | Synthetic deadline/output-loss checks passed; installed response-loss/reopen journey open |
| M05-H | Service profile changes, permissions loosen, administrator is revoked, or member generation changes | Reject stale configuration and recheck authority before binding; restart only the same owned service | Private configuration, all-choice and preflight-lock cancellation process checks passed; installed initiating logout/revocation and process-restart cleanup passed (S05/S07); authenticated worker reboot remains open |

[Installed gateway and browser results](../../../docs/reviews/2026-11-27-claude-product-connection.md) record S01–S07/U01–U03 and the pending lifecycle/provider gates. No primary row is closed by these supporting checks.

[Owner disconnect/reconnect worklog](../../../docs/reviews/2026-11-27-claude-disconnect.md) tracks D01–D06/DU01–DU03, including retained failures, synthetic Active fixtures and current-source acceptance.

[Subscription preparation and login evidence](../../../docs/reviews/2026-11-27-claude-subscription-bridge.md).
The [ten native preparation checkpoints](../../../docs/reviews/2026-11-27-claude-runtime-preparation.md)
expand M05-A/M06 without adding completed primary journeys.

## Vault account selection — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| V01 | Google connection with an empty Vault | Request Google email/password first; encrypt and save, then create fresh Kiri | Open in final portable install |
| V02 | One saved Google account; unlock the Vault | Reuse it without requesting Google password again; selected identity bound to recipient | Live pass, 2026-11-27 |
| V03 | Two saved Google accounts; choose the second | Explicit chooser; only chosen account reaches Google; no first-account fallback | Open in final portable install |
| V04 | Enter a wrong Vault password, then the correct one | Remain locked after the failure; same connection continues after correct unlock | Live pass again after production c8cf8e8 transition, 2026-11-27 |
| V05 | Use a Google email different from the Moosuri login email | Preserve member ownership while consent and read match selected Google identity | Open with real second account |
| V06 | Cancel, log out, or switch member before/during sealed delivery | No browser release for revoked identity; stale ciphertext cannot reopen a run | Open full journey; prior synthetic boundary tests retained |

## Google login and device confirmation — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| L01 | Saved valid account, fresh Kiri profile | Vault fills Google login privately; verified identity before consent | Earlier operator pass; public runtime identifier stall fixed and deployed. Fresh attempts now submit the identifier but Google rejects login before password/consent; current L01 remains blocked by provider |
| L02 | Google asks for device confirmation; approve the matching request on the connected Flip5 via Phone Agent | Wait shown in app; same Kiri attempt proceeds into service consent and connects | Live pass, 2026-11-27 |
| L03 | Decline or leave device confirmation until expiry | No connected badge or API read; bounded browser cleanup; explicit fresh retry | Open actual journey |
| L04 | Saved password is wrong | One attempt, useful failure; no repeated password submission or silent account replacement | Open actual journey |
| L05 | Provider identity differs from the selected Vault account | Refuse grant persistence and data read; keep other member's data untouched | Open full journey; prior synthetic mismatch check retained |
| L06 | Provider rejects browser, requests unsupported verification, or device is securely locked | Specific visible pause/failure; no cookie copy, browser fallback or unlock bypass | Actual Google rejection shown and auth cleanup verified after identifier fix; unsupported verification/locked-device branches remain open |

The Flip5 is an authorized operator device for this acceptance run, not a product
prerequisite for a new installer. Other users approve on their own Google device.
Phone Agent admission/reservation applies to hardware control. Only the request
matching the current account and Linux login is approved.

## First installation OAuth app — 7 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| O01 | Admin connects Google with no project, OAuth client or GWS configuration | Same-profile private SDK creates a stable unbilled project and enables exactly supported APIs | Operator experiment passed earlier; automatic route open |
| O02 | Unconfigured Auth Platform in that project | App/support/contact, External audience, scoped access and agentic web-client marker are configured; exact HTTPS callback | Fixed candidate DOM create/capture and exact persisted list/detail passed on a disposable client; installed automatic setup remains open |
| O03 | First client creation succeeds | Capture secret once into atomic private installation registration; UI/GWS observe same client; no secret in logs or model | Operator capture passed; candidate private worker/gateway/writer integration passed with synthetic Console answers; live automated route open |
| O04 | Lose response after project, branding or client create | Reconcile stable identity before any write; no duplicate project/client or false success | Linux local-save failure, lost save acknowledgement and saved-file-before-checkpoint cases passed; actual provider fault injection open |
| O05 | Client exists but creation secret was lost | Reconcile same client and supported rotation; handle two-secret limit and lost rotation response without duplication | Bounded same-client recovery integrated; live DOM one-Add/private capture/reload/two-secret refusal passed after fixing NEW/status markup; full installed recovery and two-secret loss resolution remain open |
| O06 | Admin logs out/is demoted while SDK or Console operation is pending | Stop new writes, preserve uncertain intent, verify SDK teardown before successor admission | Open confined end-to-end fault injection |
| O07 | Terms, quota, organization, branding, billing or verification blocks progress | Explain exact provider requirement and resume same setup; never silently link billing or declare production readiness | Actual missing public branding URLs observed; anonymous installation pages tested in Linux Chromium; provider publishing and other branches open |

On 2026-11-27 the actual form exposed `Use this client for an AI-powered agent`.
Google's [client guidance](https://support.google.com/cloud/answer/15549257?hl=en#agentic-client)
says automated tools/agent workflows use an agentic client, separate from ordinary
interactive API clients when both exist. The current Google work client is agentic.
Adding scope text requires **Add to table → Update → Save**, then persisted
readback. User consent still grants only the selected service's minimum scope.

## Shared client and private member grants — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| S01 | Registered app; same member connects Gmail | Same-profile consent, account match, automatic member GWS readiness | Live pass, 2026-11-27 |
| S02 | Second member connects a different Google account | Same app, separate browser/recipient/token/volume; each reads only own mailbox | Open real two-member acceptance |
| S03 | New member joins after registration | Existing app distributed automatically; no Cloud setup or admin token inherited | Open full journey |
| S04 | App is Testing and member is not a test user | Display audience restriction, never report connected; adding the explicitly chosen test user permits a new attempt | One authorized test user added; denied-member journey open |
| S05 | Submit the same app again, or a conflicting app | Exact replay preserves grants; conflicting client refused without mutation | Prior synthetic writer checks; actual full journey open |
| S06 | Installation is ready for a limited audience; member is not a test user and returns after seven days | Shared External app has the intended publishing status; provider warning/cap and revocation remain explicit; no manual test-user prerequisite hidden in the product | Official limited-audience exception confirmed; automated production transition and live acceptance open |

## Conversation and Gmail read — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| C01 | Connected member says “최근 받은 메일 5개 확인해줘” | Existing typed workflow reads one inbox metadata page and displays result in original conversation | **Failed live**: HTTP 422 `personal_text_model_unavailable`; configured Claude worker socket is absent. Typed inbox intent and UI continuation now pass synthetic Linux journeys; production worker restoration and real chat remain open |
| C02 | Same request with no Google connection | Existing Vault-first flow; consent returns to same thread/request and reads once | Typed flow tested with absent connection and reload; final-source real journey open |
| C03 | Completed read; reload and reopen the conversation | Persisted receipt and five rows visible; no additional provider read | Live pass again after production c8cf8e8 supported API read: five saved rows remained after reload, 2026-11-27 |
| C04 | Read completes but response is lost; resume same request | Existing receipt returned, same thread/request; no duplicate read | Existing completed request replay passed live (HTTP 200, same thread/request); actual response-loss injection remains open |
| C05 | “메일은 읽지 마”, a quoted instruction, or a how-to question | No Google read; ordinary response or clarification; no invented result | Positive grammar tested; six real Sonnet 5.5/medium classifier calls matched on Linux; full member-route journey open |
| C06 | Permission revoked/account switched while read is pending | No stale result exposed; connection error explained; old-account history withdrawn as defined | Synthetic stale-thread and new-account UI cases pass; actual revoked provider journey open |

The live API read on 2026-11-27 returned HTTP 200, `completed`, five metadata
items, matching mailbox, `one_inbox_page_metadata_only`, and `has_more=true`.
Reload displayed one saved receipt and five mail rows. This proves the existing
member/GWS path and persisted UI result, **not C01 natural-language routing**.
Replaying that completed request returned the same completed thread/request on
2026-11-27. A request with a different asserted member UUID returned HTTP 401.
This is a live ownership rejection check, not two real members' independent
Google consent acceptance.

## Recovery and lifetime — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| R01 | Close/reopen the member page during login or consent | Truthful saved progress; do not initiate another browser or grant implicitly | Open full journey |
| R02 | Cancel setup while SDK grant is active | Await remote revoke plus local-empty proof; teardown receipt clears only matching run | Open confined failure/cancellation acceptance |
| R03 | Revoke fails, or kill worker mid-revoke | Durable cleanup-unconfirmed fence remains beyond lease expiry; no force-unlock or duplicate SDK grant | Open actual confined injection |
| R04 | Late claim/begin arrives after browser teardown | Closed run ID cannot become active; successor remains untouched | Existing registry tests passed; full process journey open |
| R05 | GWS distribution is delayed or member runtime restarts | Pending shown until exact app readiness; recover without replacing member consent | Candidate wait is bounded from actual worker idle start; ambiguous start POST is never retried; full journey open |
| R06 | Completed connection survives gateway/member restart | Same selected account and grant work on next explicit read; no copied session cookie needed | Open actual journey |

## Completion and work order

Keep one implementation owner per checkout. Work in usable units:
(1) complete the same-profile Console/setup/registration path,
(2) close conversation/model first-use gaps,
(3) run final-source empty-install and two-member journeys,
(4) independent review and matched public/production release.
Record failure, correction and rerun against each ID; do not replace a failed
journey with a simpler API test or mark all rows passed from test totals.

## 2026-11-27 candidate correction evidence

These results do not close the final-source installation or real-provider gates.

- **I02–I04:** installed model service now creates lifecycle-derived member sockets
  before a provider is attached. The admin's first API connection and CAS key
  replacement write a dedicated private directory, outside the strict member
  registry. Independent Claude review found and corrected the original registry
  contamination defect. Bad member socket paths affect that member; failed
  enrollment reads fence dispatch until a verified snapshot returns. Missing or
  corrected key files are retried. Linux runtime tests: 24 passed; setup-file tests:
  7 passed; gateway administrator/role-demotion/crash-state case: passed; installer
  cases: 15 passed. Real API key verification/inference and final empty install
  remain open. The existing production subscription worker is still absent.
- **C01–C06:** typed inbox intent carries only the same thread and fixed metadata
  scope. The existing request ID survives interpretation, Vault continuation,
  reload and saved-read reconciliation. Unknown outcomes offer an explicit new
  read; they are never silently retried with a new ID. Independent review found
  and corrected Korean negation/compound scope gaps and stale-thread UI updates.
  Linux server interpretation cases: 13 passed. Inbox UI cases: 11 passed,
  including another account in the same browser. These use synthetic provider
  replies, not real conversational acceptance.
- **O02/O07:** fresh Kiri account login was approved on the matching Flip5 through
  Phone Agent. The fixed Console script read back actual persisted branding,
  External/Testing test-user eligibility and all seven Data Access scopes.
  Corrected the shadow-root account toolbar, labelled editable branding, visible
  accessibility panel versus setup dialog, and abbreviated scope strings. Six
  [DOM journeys](../../../audit/google-console-dom/run.mjs) cover these observed
  shapes with synthetic accounts plus foreign identity/extra-scope rejection.
  The create-client form was inspected without submission, then cancelled and
  the fresh browser destroyed. No duplicate client was created. This does not
  prove the full creation wizard or secret registration automation.
- **L03:** one operator verification attempt expired while its phone confirmation
  was pending. It returned `needs_human/verification` and discarded its fresh
  browser; a distinct fresh attempt succeeded. This is operator-helper evidence,
  not the final packaged product's timeout acceptance.

Console per-command journalling and the gateway/worker/private registration route
are implemented in the candidate. Live creation and secret-loss recovery,
real conversation, two real members, final-source reboot acceptance and matched
public/production release remain required.

Six real Claude Sonnet 5.5/medium calls on Linux used the candidate's exact
classifier policy and empty synthetic candidate list: the ordinary inbox and
no-connection wording proposed `inbox_read`; negated/quoted requests proposed
`chat`; deferred and full-body summary requests proposed `clarify`. Runtime
modelUsage confirmed the specified model. These calls had no mail or credentials
and bypassed the still-broken production member broker, so C01 stays failed live.
Integration inspection also caught a 4096 default dispatch reservation smaller
than the 6267-byte-token short inbox interpretation envelope. The broker default
now shares a 32768 reservation contract with the member interpreter's maximum-input test;
provider-measured usage still governs charges.

### Recovery corrections and independent review

At that checkpoint the matrix had **49 journeys**, 6–7 per feature; I07 below brings the active matrix to50. These remain acceptance
journeys, not a count of unit tests. Candidate Google connection tests exercise
real member HTTP reauthorization, SQLite checkpoints and the private atomic
writer with synthetic Console responses. They cover one creation, saved file
before checkpoint, missing secret, uncertain absence, logout, transient save
failure and lost save acknowledgement. Creation is never repeated to repair a
local save. Only an actual private-file absence is reported as a lost secret;
missing session/registry authority is distinct.

Independent Claude Sonnet 5.5/medium review identified premature teardown proof,
lost-secret save failures, transient polling cancellation and false list absence.
Corrections preserve the cleanup fence, retry local registration at most three
times, reconcile exact saved identity/secret under the same lease, and require
positive complete-list evidence. Linux results: gateway 36 passed, registry 21,
browser connector 122 passed/1 ignored, UI 32 and client DOM 7. The final gateway
deadline/error-classification refinements also passed all 36 gateway cases;
private root relay cases passed (2). Engine setup/settings UI passed (17),
including API-key recovery remaining accessible after initial configuration.
Web typecheck/build and plan/link verification passed. These are candidate
checks and do not upgrade open live journeys.

A fresh operator Kiri login returned `needs_human/provider_sign_in_required`;
no authenticated session was published. It did not verify client selectors or
new Google resources. The phone reservation was released without approving a
login. Resume live DOM acceptance through the authorized Vault-backed fresh
login; do not reuse a failed/stopped session or call synthetic checks live.

### Subsequent live Console and public-page acceptance

A later fresh operator login succeeded after the matching Linux device approval
on the Flip5. The candidate's fixed script read the actual existing client list
and details without mutation. Live markup exposed three defects: a leading
checkbox header shifted the Name column; the detail used a labelled editable
Name and a type heading; an `aria-hidden` loading placeholder remained after
loading. Corrections preserve exact account/project/name/callback/agent checks.
Linux client state-machine tests passed (8), and DOM fixtures passed (13).

The same script completed the actual create form's six preparation actions
(type open/select, name, URI add/fill and agent check), then stopped at Create.
The draft was cancelled. No duplicate client or new secret was created. The
empty-project list and actual creation-result/rotation capture remain unproved.
This operator session is not the final packaged Vault-first journey.

Google's actual Audience page refused Publish app with a Branding prerequisite.
The saved homepage, privacy and terms fields were empty. The candidate now
serves `/about`, `/privacy`, `/terms` anonymously from its own origin. Linux
Chromium at 390px and 1280px passed direct entry, navigation/reload, keyboard,
overflow and no-member/API/provider-request checks; gateway route restrictions,
five React cases and web typecheck/build also passed. The tests are in
[the public information browser harness](../../../deployment/kiri/linux-vm-smoke/public-information.py).
An independent Claude Sonnet 5.5/medium review found omitted Calendar write
disclosure; source inspection also identified separate mail-write consent. The
pages now distinguish those permissions and final approvals from read-only
connections. This does not prove Google publishing or verification.

Opening four Console tabs in one operator capsule exhausted its Chrome memory;
that capsule was stopped. Subsequent inspection used one Console tab and the
successful session was explicitly stopped after use. This is an operator
observation, not evidence that the product's single-tab flow failed.

### Publication boundary and installer regression

The candidate's separate Domains/Publish journal passes Linux state/recovery
checks and gateway command checks. A real authenticated HTTP test covers all
seven consent-start paths while managed setup is incomplete, plus unreadable
registry failure. S06 still requires live Google publication, and no member
journey is upgraded solely from these component results.

I01 exposed a clean-install packaging failure at the member Docker build:
the embedded model-service unit was outside the Docker context allowlist. The
exact file is now included. Final source installation, the second member and
reboot must still run after this correction; the failed run is retained.

### Existing installation transition

The active matrix now contains **50 journeys**, 6–7 per feature. I07 was added
after checking the actual production host: Ubuntu 22.04/kernel5.15/Kiri0.2.1,
without a setup-kiri receipt. A successful new Ubuntu 24.04 installation does not
prove that upgrade. Keep its encrypted Vault, member allocations and Google
grants while validating a matched component transition; the fresh installer
must not be forced over the existing installation or used to upgrade the host OS.

### Same-client recovery and current-kernel rollout acceptance

O05 now reserves a fresh administrator/lease-bound recovery attempt only when the
private registration is absent. Setup/client identity and a three-attempt bound
survive restart. Create and Recover share a worker's single provider-write budget.
A second authorized save between reservation and recovered response is retained
byte-for-byte; the recovery save is refused. Store43, gateway38 and the added
competing-save scenario passed on Linux. DOM17 passed with synthetic values.

A live disposable-client run exposed two actual markup differences: NEW is inside
the Client secret DT, and after Add each Enabled status includes a Disable control.
The fixed expression captured the new secret privately, confirmed the unchanged
old row plus the new suffix after reload, and refused another Add before/after
reload. The client was then deleted through both Google confirmation dialogs and
confirmed absent; private captures were removed. The original app/grants were
unchanged. This proves the DOM subflow, not an installed controller completion.
Two preexisting secrets or an unrecoverable response still pause without deletion.

I07: a separate Ubuntu 22.04 guest booted the actual production kernel
5.15.0-179 with Kiri 0.2.2. Public invitation/member/daemon recovery checks passed;
Chromium and Google Chrome both passed the actual installer startup/confinement/
owned-cleanup probe. The new [owned rollout harness](../../../deployment/kiri/linux-vm-smoke/guest-owned-rollout.py)
then passed six actual checks: lifecycle-lock refusal, wrong-container-ID refusal,
owned profile replacement with unchanged UID/volume/keys/database/reservation,
unaffected sibling PIDs, same-target retry, and matched lifecycle resumption.
The image content stayed at116a8cd; a different final image and production are
still unverified. No host OS upgrade or installation-receipt adoption was done.

### Post-transition production checkpoint

The matched c8cf8e8 transition preserved the existing real member and Google grants.
Post-transition API inbox read returned five metadata items; the original thread
retained five rows after reload. Wrong/correct Vault password and visible managed
model-recovery form were checked. None of this closes C01: the provider connection
is still unavailable. A fresh Drive connection reached the Vault but then failed;
a public-proxy default-route defect was reproduced and corrected in the installer.
Six real Linux egress/isolation/restart checks passed. Production proxies were repaired and real HTTPS/private refusal rechecked. The new
login did not complete; explicit cancellation cleaned its browser/socket. Full
login results are recorded in [the production log](../../../docs/reviews/2026-11-27-matched-production.md).
