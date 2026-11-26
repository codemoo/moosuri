# Matched production transition and Google egress acceptance

## Retained installation

On 2026-11-27 the existing Ubuntu 22.04/kernel5.15.0-179 installation moved to
Kiri 0.2.2 and public source c8cf8e8. Its real member, dedicated UID, mounted volume,
logical databases, encrypted Vault and protected keys were preserved. Installation
roles, model bindings, shared Google app and private member grants were unchanged.
The same-target member rollout retained the new container. Six matching services
and certificate-verified assistant/about/Vault HTTPS passed. No host OS upgrade,
new installation receipt, provider mutation or model inference was part of the switch.

Consistent private backups and before/after receipts remain on the Linux host.
The operator did not repeat the transition after the startup check encountered a
Type=simple pre-exec PID: the original stopped member and data were checked, then
only the remaining broker/member/service stages resumed. The public harness now
counts matching executable samples toward its stable readiness interval.

## Actual member checks after the transition

- Kiri retained the authenticated Moosuri session and existing Gmail connection.
- The managed Claude recovery form opened with API credential/model/effort fields;
  no key was submitted or connection replaced.
- Wrong Vault password stayed locked; correct password opened the retained Vault
  and existing Google item. No Google password or recovery key was exposed.
- A real authenticated inbox request returned 200/completed, five metadata items,
  the selected mailbox and matching request/thread. Five rows remained after reload.
  This is the direct member/GWS route, not natural-language interpretation.

## Fresh connection found a missed network boundary

The public Drive connection opened a fresh recipient and Vault handoff, then
returned unavailable. Its proxy had both endpoints and working DNS but no default
route; HTTPS CONNECT to Google's public login entrypoint returned 502. Previous
empty-profile startup/confinement probes did not exercise external connectivity.

In a separate installed Ubuntu 24.04/Kiri 0.2.2 fixture, public-primary proxy creation
followed by exact-IP internal attachment produced a default route and Google 302.
The [real egress harness](../../deployment/kiri/linux-vm-smoke/guest-proxy-egress.py)
then passed six checks: public default route, actual Google HTTPS, loopback/metadata
refusal, internal-only worker without a default route, restart with usable egress,
and owned cleanup while retaining the original configuration. No login or account
credential was used in these checks.

The source installer and matched rollout helper now use this sequence. The
installer refuses an old internal-primary proxy or unexpected network and checks
the actual kernel route before admitting it. Six targeted installer tests passed
on Linux, including interrupted preparation and create/attach/start ordering.
Independent official Claude Sonnet 5.5/medium reviewed the implementation and
operator repair; ownership, recorded evidence and interruption recovery were
checked before production application.

Production now uses two separately prepared public-primary proxies. Both passed
real Google 302, private-address 403, empty browser confinement and owned cleanup,
then restart and repeat network checks before admission. Configs and matching smoke
evidence were switched with preserved before-images and an explicit rollback path.
Matching gateway/browser executables stayed stable for ten seconds, real HTTPS
returned 200, and the existing member ID/PID/mounts, protected keys and shared client
were unchanged. The old idle proxies remain running for explicit rollback; they
are no longer selected by either configuration.

The next actual Drive attempt decrypted the Vault account and reached the opening
state. It did not reach a verified login or consent completion. Phone Agent was
reserved under a valid admission decision; the device displayed a same-account
Linux confirmation that was expired when acted on. No successful device approval
is claimed. The app continued showing opening, so the attempt was explicitly
cancelled. The app showed cancellation, the root socket had zero connections, no
activated authentication unit remained, and each current project contained only
its proxy. The device reservation was released. Provider/page-state diagnosis and
a new successful login remain open; do not repeat uninstrumented attempts.

## Invitation preparation

The existing public onboarding worker is now enabled as a one-minute production
timer. Its private policy reserves up to three unused UIDs for subsequent members,
with the existing 10GiB per-member profile. The empty queue ran successfully
and the existing lifecycle configuration/member stayed unchanged. This does not
claim a real second production member was invited or connected. No messages were
sent. Physical capacity is still checked by the existing storage provisioner.

An old, stopped disposable browser fixture was compressed after proving no process
held its disk. The resulting qcow2 passed its format check and a full logical-disk
comparison before atomic replacement, recovering about 7GiB without deleting the
fixture or changing production storage. The current two-member fixture is intact.

## Remaining acceptance

The 50-journey matrix remains open for final-source empty installation/default
entrypoint, real production invitation fulfillment, managed first OAuth setup/publication
and recovery, two real member Google consents, and an authorized working model
connection plus natural-language Gmail. The preserved subscription worker is
unavailable. Direct GWS success is not a substitute for that chat acceptance.

[Journey matrix](../../openspec/changes/automate-installation-google-app/journeys.md) ·
[Work log](../../openspec/changes/automate-installation-google-app/tasks.md)
