# Public source installation and preserved production admission

## Exact installed source and recovery

An initially empty Ubuntu 24.04 guest cloned public source
`dee8475486ee366567db219ec7a2ac307928abc8`. Its baseline had no Moosuri,
Kiri/Docker, Rust/Node, GWS, gcloud or provider credentials. The public source
entry built the release and ran the product's real terminal prompts for separate
HTTPS origins, administrator identity and hidden password confirmation. Supplied
synthetic certificates were used; real DNS/ACME issuance was not tested.

The host ran out of storage during installation. The original failed acceptance
and incomplete installation receipt were retained. After host storage recovery,
the same complete bundle and same answers resumed the original installation;
every receipt field except `complete` was unchanged. This is an empty-start
installation with tested interruption recovery, not an uninterrupted success.
No source patch, replacement identity or provider credential was introduced.

The durable recovery acceptance exited zero and recorded the exact source:

- HTTPS assistant and separate Vault, administrator login and single-use handoff.
- Same-answer replay and refusal of a conflicting origin without identity/key changes.
- A second member completed the public invitation API with their own password;
  the root CLI did not initialize that password. The installed timer retained the
  original allocation. Replay and administrator-role boundaries were checked.
- Both members used actual Chromium to create an eight-character Vault, save an
  encrypted synthetic item, reject a wrong password and reopen their own item.
  Mobile and desktop layouts were inspected; the mobile Vault had no overflow.
- Separate Kiri recipients, cross-member refusal, cancellation and logout.
- Actual guest reboot, verified by a changed boot ID, followed by both members'
  login, Vault reopen, saved-item isolation and administrator boundary checks.
- Six network checks: public default route, real unauthenticated Google HTTPS,
  private-address refusal, no external route in the private worker, proxy restart
  and owned cleanup without changing installed configuration.

The model service correctly remained unconfigured. No real Google login, OAuth
creation, model inference or real certificate issuance is claimed. Earlier
[J01–J08 browser and four-member acceptance](2026-11-27-web-invitation.md)
continues to cover invitation faults; this latest run binds clean installation,
web credential completion and reboot to the final runtime source.

## Production transition

The accepted guest's complete bundle was copied with every artifact hash checked.
The operator, helper and probe were pinned to the independently reviewed versions.
Both new browser capsules passed actual confinement/admission/cleanup probes on
the existing Ubuntu 22.04/kernel 5.15.0-179/Kiri 0.2.2 production host before
service admission changed.

The transition stopped all registry consumers, installed persistent admission
holds and saved a consistent private backup. It switched the matching host
services, member image, browser images and onboarding integration together.
The existing member UID, mounts, logical databases, encrypted Vault, allocations,
protected keys, installation identities, model bindings, shared Google app and
member grants were compared and retained. The member confinement settings stayed
unchanged. Same-target rollout retained the replacement container. All six host
services ran matching executables stably, SQLite checks passed and certificate-
verified assistant/about/Vault HTTPS returned 200. The existing onboarding timer
and its policy were retained; the retired Google socket remained disabled/fenced.
No host OS upgrade or new-installation receipt was fabricated.

Production now runs runtime source `dee84754` and member image
`sha256:c9fdfbea39313e4f5a299021c9f2f63f0c3edcd1688419b8fb5c19886027c188`.
Private backups and receipts remain on Linux. The operator completed with exit 0.

## Post-transition member checks and limits

After reloading the authenticated member page in Kiri Chrome:

- The original session and Gmail connection remained usable. The Vault handoff
  opened its separate origin at the existing-Vault unlock screen without error;
  this check did not decrypt the live Vault again.
- An authenticated GWS inbox request returned HTTP 200, five metadata rows,
  the selected mailbox and `one_inbox_page_metadata_only`.
- A foreign asserted member and an invalid invitation token each returned 401.
- The administrator setup API returned 200/supported. The member-management
  screen displayed invitation controls. No production invitation was issued.

Mail fields, tokens, capability values and configuration bodies were suppressed
from the evidence. The inbox check did not invoke a model and does not close C01.
There is no new production reboot claim; real reboot acceptance above concerns
the separate installed guest.

The existing owner-bound Claude configuration still has no working subscription
worker. An independent official Claude Sonnet 5.5/medium source trace confirmed
that the worker and terminal login exist, but Kiri setup creates only API-backed
connections: worker account/profile/unit/binding provisioning and a product login
handoff are missing. The trace made no code changes or production/provider calls.
It is not a successful recovery. Developer/reviewer authentication was not reused.

The fresh Google rejection recorded in the
[previous production report](2026-11-27-matched-production.md#live-identifier-correction-and-provider-rejection)
has no changed prerequisite, so no repeated login or stale phone prompt was used.
Installed automatic OAuth creation/publication/recovery, two real members' Google
consents and real natural-language Gmail remain open in the
[61-journey matrix](../../openspec/changes/automate-installation-google-app/journeys.md).
