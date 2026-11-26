# Work log

- [x] Reconcile scope with a clean public installation and no pre-existing Google resources.
- [ ] Package installation/runtime/browser/vault/GWS dependencies without developer files or fixed owner routing.
- [ ] Observe official Console steps in fresh Kiri and identify provider prerequisites.
- [ ] Implement resumable administrator setup and direct client registration.
- [ ] Integrate same-browser member consent and automatic GWS readiness.
- [ ] Validate Linux scenarios, obtain independent Claude review, deploy and publish.
- [ ] Complete real OAuth registration, member consent and one GWS Gmail read.

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
