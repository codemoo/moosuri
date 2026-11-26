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
journey. All model execution and independent review uses Claude Sonnet 5.5 / medium.

## Install and first use — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| I01 | Empty server; obtain public source and follow the default installer | HTTPS app and separate Vault work without private helpers, preinstalled Kiri/GWS or developer credentials | Open: final-source clean install |
| I02 | First admin opens the installation without a model connection | Clear admin setup; no provider calls, guessed model or local fallback | Candidate admin-only API form, empty state and file lifecycle passed on Linux; final install open |
| I03 | Admin attaches an authorized model connection and selects Sonnet 5.5 / medium | First real chat and work interpretation use that selection; credentials stay private | Open: first-provider setup and current interpretation failure |
| I04 | Invite a second member after all services are already running | Independent UID/volume, browser/Vault/GWS and model sockets appear without manual service editing | Diagnostic install pass; dynamic first/later member model sockets passed on Linux; final install open |
| I05 | Stop installation after preparation, then run the same installer | Original installation, member allocation and keys retained; conflicting inputs refused | Diagnostic pass; final install open |
| I06 | Reboot a completed installation with two members | Same identities and connections, usable app/Vault and services; no duplicated requests | Diagnostic pass; final install open |

## Model first use and recovery — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| M01 | Empty installation; admin registers the shared Anthropic API connection | Both first members use Sonnet 5.5/medium; API credential stays in the host's private setup directory | Linux synthetic setup/socket boundaries passed; real API credential and inference open |
| M02 | Invite another member during an existing member's model request | New socket appears; existing request is not cancelled or charged twice | Dynamic socket isolation passed; full install journey open |
| M03 | Wrong/revoked key, unsupported model or invalid effort | Show the actual unavailable state; preserve an existing connection until the replacement is valid | Catalog rejection/CAS tested; real inference/effort and administrator diagnostics open |
| M04 | Lose API-setup reply, then another administrator changes the connection | Reconcile this request's revision; never attribute another administrator's save to this request | Candidate UI attribution and atomic-save tests passed |
| M05 | Administrator elects an official Claude subscription connection | Provision isolated non-root worker, fresh login and exact member binding from public source; no copied developer authentication | Missing production worker and public provisioning automation remain open; CLI prerequisites traced independently |
| M06 | Worker is missing/restarted, member generation changes, or server reboots | No borrowed member login, no fallback model, no new charge before dispatch; truthful recovery | Socket isolation/replay boundaries passed; fresh subscription/reboot journey open |

Shared model credentials use the installation API route. The existing official
Claude subscription worker is pinned to one member and authenticated principal;
it cannot serve a second member by copying its binding or credentials. Source
inspection is not evidence of successful fresh provider login or inference.

## Vault account selection — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| V01 | Google connection with an empty Vault | Request Google email/password first; encrypt and save, then create fresh Kiri | Open in final portable install |
| V02 | One saved Google account; unlock the Vault | Reuse it without requesting Google password again; selected identity bound to recipient | Live pass, 2026-11-27 |
| V03 | Two saved Google accounts; choose the second | Explicit chooser; only chosen account reaches Google; no first-account fallback | Open in final portable install |
| V04 | Enter a wrong Vault password, then the correct one | Remain locked after the failure; same connection continues after correct unlock | Live pass, 2026-11-27 |
| V05 | Use a Google email different from the Moosuri login email | Preserve member ownership while consent and read match selected Google identity | Open with real second account |
| V06 | Cancel, log out, or switch member before/during sealed delivery | No browser release for revoked identity; stale ciphertext cannot reopen a run | Open full journey; prior synthetic boundary tests retained |

## Google login and device confirmation — 6 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| L01 | Saved valid account, fresh Kiri profile | Vault fills Google login privately; verified identity before consent | Live pass, 2026-11-27 |
| L02 | Google asks for device confirmation; approve the matching request on the connected Flip5 via Phone Agent | Wait shown in app; same Kiri attempt proceeds into service consent and connects | Live pass, 2026-11-27 |
| L03 | Decline or leave device confirmation until expiry | No connected badge or API read; bounded browser cleanup; explicit fresh retry | Open actual journey |
| L04 | Saved password is wrong | One attempt, useful failure; no repeated password submission or silent account replacement | Open actual journey |
| L05 | Provider identity differs from the selected Vault account | Refuse grant persistence and data read; keep other member's data untouched | Open full journey; prior synthetic mismatch check retained |
| L06 | Provider rejects browser, requests unsupported verification, or device is securely locked | Specific visible pause/failure; no cookie copy, browser fallback or unlock bypass | Open full journey |

The Flip5 is an authorized operator device for this acceptance run, not a product
prerequisite for a new installer. Other users approve on their own Google device.
Phone Agent admission/reservation applies to hardware control. Only the request
matching the current account and Linux login is approved.

## First installation OAuth app — 7 journeys

| ID | Starting condition and action | Observable acceptance | Current result |
| --- | --- | --- | --- |
| O01 | Admin connects Google with no project, OAuth client or GWS configuration | Same-profile private SDK creates a stable unbilled project and enables exactly supported APIs | Operator experiment passed earlier; automatic route open |
| O02 | Unconfigured Auth Platform in that project | App/support/contact, External audience, scoped access and agentic web-client marker are configured; exact HTTPS callback | Live Console experiment passed; automatic route open |
| O03 | First client creation succeeds | Capture secret once into atomic private installation registration; UI/GWS observe same client; no secret in logs or model | Operator capture passed; candidate private worker/gateway/writer integration passed with synthetic Console answers; live automated route open |
| O04 | Lose response after project, branding or client create | Reconcile stable identity before any write; no duplicate project/client or false success | Linux local-save failure, lost save acknowledgement and saved-file-before-checkpoint cases passed; actual provider fault injection open |
| O05 | Client exists but creation secret was lost | Reconcile same client and supported rotation; handle two-secret limit and lost rotation response without duplication | Open implementation and actual Console observation |
| O06 | Admin logs out/is demoted while SDK or Console operation is pending | Stop new writes, preserve uncertain intent, verify SDK teardown before successor admission | Open confined end-to-end fault injection |
| O07 | Terms, quota, organization, branding, billing or verification blocks progress | Explain exact provider requirement and resume same setup; never silently link billing or declare production readiness | Testing and incomplete-branding restriction observed; remaining branches open |

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
| C03 | Completed read; reload and reopen the conversation | Persisted receipt and five rows visible; no additional provider read | Live pass after supported API read, 2026-11-27 |
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

The matrix now has **49 journeys**, 6–7 per feature. These remain acceptance
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
