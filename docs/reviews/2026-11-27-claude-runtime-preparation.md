# Credential-free Claude runtime preparation

This follows the [private login bridge](2026-11-27-claude-subscription-bridge.md)
within M05/M06. It prepares an owner's isolated Linux resources and checks a
prospective launch. It does **not** log an account in, start the model worker,
write a member binding, complete the Kiri UI or close conversational Gmail C01.
Production remains dee84754 and its model configuration is unchanged.

## Implementation boundary

The internal `claude-runtime-prepare` command reads a private root-owned request
containing only a request UUID and the complete owner tuple. All service names,
UIDs, paths, downloads and image pins derive from trusted code. Ubuntu24 x86_64,
cgroup2, rootless tools and automatic system-user subids are prerequisites. A
host lacking the supported useradd capability, including the existing Ubuntu22
installation, is refused before preparation writes. This is an open portability
gate; the host OS is never upgraded by this command.

The command verifies the unchanged official Claude2.1.322 artifact, the pinned
rootless wrapper and immutable image. A root-only receipt precedes account
creation; exact replay retains the same UID/GID/subid ranges. Worker home files
are accessed through checked directory descriptors. Foreign users, receipts,
units and artifacts are rejected. The actual service UID must be able to read
and execute each pinned executable. The public host bootstrap includes the
rootless distro packages; the preparation command does not invoke apt.

Authentication storage reuses the member filesystem implementation in a separate
root. Each preparation request has an independent preallocated128MiB ext4 image,
32768 explicitly formatted inodes, private auth/scratch directories, and an exact owner/UID/GID
claim. Its filesystem UUID is the preparation request; the private claim retains
the distinct member volume UUID. No personal registry files or member storage
are reused as Claude authentication storage.

The credential-free `claude-engine-isolation` child uses the prospective profile
and the real launch flags. It checks the native cgroup values, UID namespace,
no-new-privileges, seccomp, dropped capabilities, read-only mounts, inaccessible
host/sibling paths, direct-network denial, proxy destination denial, filesystem
bounds and the official CLI version. A passed receipt is **launch evidence**.
The root parent must still retain it with the request/owner and recheck its hash,
exact candidate inputs, current authority and Unix worker before any login/model
binding. The legacy profile parser's64-hex operator-attestation field alone is
not that root authorization. This remains a required integration gate.

A stable reserved version8 probe environment and one persistent lock permit
same-owner cleanup/retry without accumulating a new environment and lock each
time. Container cleanup must be acknowledged before erasing its filesystem.
Admission refuses an auth root containing a login environment; it does not sweep
existing credentials. Locks are not unlinked under potential waiters. All version8 UUIDs are now
rejected by normal login, discovery, inference, retirement and worker-binding
paths before their side effects.

## Journeys and first failure

The [marked-VM harness](../../deployment/kiri/linux-vm-smoke/claude-preparation.py)
expands M05-A and M06 into ten preparation checkpoints. It uses synthetic
owners and no account credentials, login code, model prompt or inference.

| ID | Action | Required observation | Result |
| --- | --- | --- | --- |
| P01 | Prepare on an otherwise unconfigured owner | Download/verify artifacts; create a distinct service and bounded storage; report only prepared | Passed on actual Ubuntu24 |
| P02 | Replay the same request | Same owner, UID/GID, paths and image; no duplicate account | Passed on actual Ubuntu24 |
| P03 | Change owner generation or inject a path | Refuse before adopting another owner's preparation | Passed on actual Ubuntu24 |
| P04 | Check the exact prospective launch | Kernel confinement and cleanup, with no account authentication | Initial run failed before container launch: the Unix socket path was142 bytes, above Linux's107-byte payload limit. Corrected by removing the duplicated UUID path segment while retaining full owner binding; native retry passed in33.29s |
| P05 | Fill authentication storage with bytes | ENOSPC within the bound; original sentinel retained; cleanup permits reuse | Passed on actual Ubuntu24 |
| P06 | Exhaust file count, then retry | ENOSPC within the inode bound; cleaned filesystem can run admission again | Actual inode exhaustion and native admission after cleanup passed |
| P07 | Modify the worker service file | No silent adoption or service restart; exact restored file permits retry | Passed on actual Ubuntu24 |
| P08 | Actually reboot, then replay and admit | Changed boot ID; same service/filesystem/owner and usable confinement | Changed boot ID, exact replay, native admission and second-owner replay passed |
| P09 | Prepare a second owner and replay after reboot | Distinct UID/storage; foreign read refused; identity retained | Passed before and after actual reboot |
| P10 | Host uses a different inode ratio; attempt login with reserved probe UUID | New filesystem stays at32768 inodes; admission works; reserved login rejected before private-file/TTY access | Passed on final candidate in55.49s; host configuration restored |

The failed Unix-socket run is retained. A separate native diagnostic confirmed
actual memory805306368, CPU100000/100000, PIDs32, no external network interface,
and a32768-inode bounded filesystem; those observations did not override the
failed full probe. No unchanged Google login was retried.

## Review, checks and next work

Independent official Claude Sonnet5.5/medium initial review and closure found
executable-traversal and probe-recovery gaps. Main added worker-side executable
checks, a stable probe identity and acknowledged container cleanup. The closure
also identified host-dependent inode formatting and a reserved-UUID collision
with login. Explicit mkfs `-N32768` and normal-path version8 rejection fix these.
A separate16-turn focused Claude review found no material introduced defect in
those two fixes. Reviews were static; Linux execution evidence comes from the
main's actual commands and marked guest.

Final Linux checks passed **63 tests**: 15 app (11preparation, 3storage claim,
1private email) and48 connector tests. The native app build passed. Earlier
scoped Clippy exited0 with warnings; this is not a warning-free workspace claim.
The first integration rerun failed because artifact access preceded rejection of
a changed service unit. The order was corrected and the final app tests passed.
Global formatting still finds unrelated pre-existing differences; changed Rust
modules were formatted without rewriting other product modules.

P01–P09 use candidate SHA256
`ac0e659dfe367ac07fbacff42236cdf37f5fe747f4ee7aa268a5a483b569c1cf`.
The final inode/UUID fixes use
`1b219560518d06a3e5308b347bf20481634839f21483ffe7adfb51aa5b7ceef0`.
The later P10 check covers the changed boundaries; earlier evidence is not
relabeled as having run on the final binary. The original failed socket run and
all later observations remain in the private synthetic VM result record.

### Required product integration gates

These remain open and prevent treating this command as a completed subscription
setup or deploying the new path for real credentials:

- Retain native evidence under root ownership, bound to request, full owner,
  exact canonical profile/artifacts and storage claim. Recheck current admin,
  session, generation, model revision and actual Unix peer before publishing a
  binding. A worker's unsigned digest is not root authority. Invalidate it when
  relevant host/engine/image state changes.
- Serialize admission, cleanup and login across the whole prepared owner root.
  The probe's per-environment lock alone does not exclude a concurrent login.
- Bound the rootless engine home/image store and system-user resources, cap
  preparation allocation, and implement exact owned retirement. The current
  128MiB bound covers only authentication and scratch, not the engine home.
- Repair interrupted storage record writes safely. The shared storage helper
  currently refuses any pending `.next` record; this can block other owners
  until inspection. Do not discard claims, reformat existing filesystems or
  call this automatic recovery complete.
- Support the existing Ubuntu22 host without upgrading its OS or pretending its
  `useradd` has Ubuntu24's system-subid option. Partial system-account/subid
  preparation also needs a documented owned repair path.
- Exercise actual official account login and model discovery/inference within
  the limits. `claude --version` and URL/cancel do not prove successful login or
  inference at32PIDs, 768MiB and the current tmpfs limits.

Next is one product unit: prepare/admit through the authenticated gateway,
retain request-bound evidence, present official login in the same member Kiri,
then verify and publish the existing owner-bound worker. Authority changes,
missing results, cancellation, reboot and a second distinct owner must use the
existing registry and worker protocols. No parallel authentication subsystem,
manual JSON prerequisite or borrowed reviewer login is acceptable. Existing
provider rejection and pending real model authorization remain separate limits.
