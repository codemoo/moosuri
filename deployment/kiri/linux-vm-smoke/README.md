# Kiri 0.2.1 disposable Linux VM smoke

This is a bounded synthetic engine experiment for `isolate-account-runtime`,
reviewed against moosuri baseline `c3a2f9c`. It is not a moosuri runtime,
production installer, migration tool, full recovery test or security certification.
Run these root commands **inside a disposable VM only**. The scripts require
`/etc/moosuri-kiri-lab-marker` to contain `SYNTHETIC_DISPOSABLE_VM` and refuse to
overwrite existing synthetic account filesystem images. They modify only the
disposable guest's filesystem and systemd units. No host folders, SSH agent,
production data, model credentials or account secrets are shared with the VM.

## Pinned inputs and environment

- Official Kiri tag `v0.2.1`: source commit
  `b249eccc2ed708ce10d7feeb82b6c0869b112443` (annotated tag object
  `560583915c1aec21f078378fb227037fd6856a3c`). Apache-2.0, package also
  contains a 394695-byte bundled NOTICE.
- `https://github.com/libracoder/kiri/releases/download/v0.2.1/kiri-lite_0.2.1_amd64.deb`
  SHA256 `5e7b1f6d8239aa590b314fd2ccc8f1472ba1427f414b35efa86ccedd6a6571b2`.
- Runtime image
  `python@sha256:9ef1a599f863231c3d91e941389bfdc4a2d6070557243d54df1a4e57015b960e`,
  amd64 image config
  `sha256:67fd5f7b4b120eaa3308775fe87aff98a2492287576bd6656ef785d54a1b5605`.
  The authoritative exact config ID is also asserted in `socket-storage.py`.
- Observed guest: Ubuntu 24.04.5 LTS, kernel `6.8.0-147-generic`, systemd,
  unified cgroup v2, x86_64; 2 vCPU, 2048MiB RAM, 10GiB private qcow2.
- Ubuntu public cloud image used:
  `https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img`,
  SHA256 `04e8f2461a16a44e78a370284471f29616cdd9baf1893df25807b71f985b6c94`.
  `current` is mutable: preserve or independently verify these exact bytes;
  a later checksum mismatch requires a new recorded environment.

A real KVM VM was booted with private qcow2 and seed ISO, no shared folders,
2 CPUs/2GiB, user-mode networking, one **loopback-only** SSH forward, no agent
forwarding. QEMU was extracted under a private temporary directory; no hypervisor
or Kiri package was installed on the shared host. On another lab host use its
existing approved VM tooling. Ordinary container-in-container execution is not
substitute evidence for this Linux host engine.

## Guest execution

Copy this directory into the disposable guest. Establish the marker during
cloud-init provisioning; do not place it on any production or shared Linux host.

```sh
printf 'SYNTHETIC_DISPOSABLE_VM\n' >/etc/moosuri-kiri-lab-marker
curl -fL https://github.com/libracoder/kiri/releases/download/v0.2.1/kiri-lite_0.2.1_amd64.deb -o /tmp/kiri-lite.deb
printf '%s  %s\n' 5e7b1f6d8239aa590b314fd2ccc8f1472ba1427f414b35efa86ccedd6a6571b2 /tmp/kiri-lite.deb | sha256sum -c -
dpkg -i /tmp/kiri-lite.deb
# The package installs an inactive service. Start only in this disposable VM.
systemctl enable --now kiri
curl -fsS --unix-socket /run/kiri/docker.sock http://localhost/version
curl -fsS --unix-socket /run/kiri/docker.sock -X POST 'http://localhost/images/create?fromImage=python&tag=sha256:9ef1a599f863231c3d91e941389bfdc4a2d6070557243d54df1a4e57015b960e'
sh guest-storage.sh
python3 socket-storage.py setup
python3 socket-storage.py quota
python3 guest-lifecycle.py
sh guest-mount-failure.sh
python3 socket-storage.py verify
systemctl reboot
# Reconnect after the guest has actually rebooted:
python3 socket-storage.py verify
```

The storage setup gives each UID a separate hard-cap 32MiB ext4 filesystem, mounted
`nodev,nosuid,noexec`. This is a bounded filesystem-cap example, not a scalable
production storage manager. `RequiresMountsFor` and explicit `mountpoint` checks
prevent Kiri startup on an empty fallback directory when an account mount fails.
Because the whole engine depends on both lab mounts, a missing mount blocks the
whole engine; product account provisioning must define its own availability policy.

The Python helpers use Docker Engine API v1.47 and correctly consume HTTP101
hijacked raw streams for exec output. A generic HTTP response body reader otherwise
sees an empty body and may inspect a still-running exec with `ExitCode:null`.

## Product web invitations in a fresh guest

The product onboarding check uses a separate fresh Ubuntu guest with the same
disposable marker. Install the candidate at `/usr/local/bin/moosuri`, load its
matching member runtime image into Kiri, and install the repository's
`moosuri-kiri-onboard.service` and `moosuri-kiri-onboard.timer` units. Run
`systemctl daemon-reload` and pass the loaded image's full SHA-256 ID to
`sudo python3 guest-web-invitations.py "$MEMBER_IMAGE_ID"`. The fixture creates
the installation, issues two synthetic web invitations and waits for the
shipped timer's first activation. It checks both logins, password rotation,
capacity, retries and installation identity. [November 21 results](../../../docs/reviews/2026-11-20-kiri-scenarios.md#관리자-웹-초대와-호스트-자동-준비)

### Kiri 0.2.2 member recovery

The same public invitation harness accepts `--recovery` to verify unchanged
reconciliation, recovery of one member without restarting its sibling, and an
engine restart with member-bound sessions and complete inspected profiles intact.
It requires a fresh marked guest and refuses the engine restart if unrelated
containers exist. Build or obtain the member image through the public
[Dockerfile](../Dockerfile), load its exact ID into Kiri, and install the candidate
host binary and stock onboarding units as above. This harness prepares its own
synthetic installation; it is not the public product installer.

```sh
sudo python3 guest-web-invitations.py "$MEMBER_IMAGE_ID" --recovery
```

[November 26 results, cold-create correction and remaining integration](../../../openspec/changes/automate-installation-google-app/tasks.md#2026-11-26-member-engine-compatibility)

## Member task history and recurring searches on Linux

Run this check as a regular Linux user with Python Playwright and its Chromium and WebKit browsers installed. From the repository root, build the web assets before the executable so that the gateway serves the updated UI.

```sh
(cd web && npm ci && npm run build)
cargo build --locked -p moosuri
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py target/debug/moosuri --channel chromium --workflows
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py target/debug/moosuri --channel webkit --workflows
```

The runner creates two temporary member volumes, an HTTPS gateway, a model broker with a synthetic reply, and a real installation timer. It drives the browser from chat through a source read, runs a scheduled search during logout, and checks which results remain visible after forgetting the source. Set `MOOSURI_SYNTHETIC_WORK_SCREENS` to an output directory to capture the mobile list, timeline and source, and the desktop split view. The `--claude` mode uses the installation's authenticated Claude Sonnet 5.5 at medium effort for the separate conversation checks. [Work log and scenarios](../../../docs/reviews/2026-11-21-member-workflows.md)

## Conversation work

The `--conversation-work` mode sends one natural-language request with two searches through the ordinary composer. It checks member-owned child records and timer receipts, targeted follow-ups, persisted questions, saved reports, source withdrawal, fresh searches and account isolation. It runs on a disposable Linux member installation with Chromium or WebKit.

```sh
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py /absolute/path/to/moosuri --conversation-work --channel chromium
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py /absolute/path/to/moosuri --conversation-work --channel webkit
```

## Chat work

Build the member web assets first, then build the Linux binary with `cargo build --locked -p moosuri --features test-fixtures`. Run `member-web-fullstack.py /path/to/moosuri --channel chromium` with one of the following options. Each invocation provisions fresh members and a private gateway. For WebKit use `--channel webkit`.

| Options | Verified path |
| --- | --- |
| `--semantic-work` or `--semantic-work --claude` | Everyday research, combined report, quoted evidence, local watches and briefs |
| `--supplemental-work` or `--conversation-work` | Preferences, separate tasks and their follow-ups, recovery and source withdrawal |
| `--live-journey daily`, `continuity`, `privacy`, `library`, `work` or `boundaries` | Coffee and meeting preferences, a long trip conversation, two members with opposite preferences, memory editing and deletion, a delegated report, and distinguishing questions from lasting preferences or watches. Every interpretation and ordinary reply uses the signed-in Claude CLI. |
| `--live-journey groceries` or `--live-journey timetable` | Revise household and camping lists, reopen the original correction as a search task, or choose courses from member-pasted adviser and timetable excerpts around a changing pickup commitment. Uses Claude Sonnet 5.5 at medium reasoning for every model call. |
| `--price-watch` | Follow an actual member timer through threshold crossings, repeats, uncertain hotel rates, a different room and partial observations. Only the hotel pages, their broker and the due times are fixtures. Verifies zero model calls on the supported price routes. |
| `--live-journey routing`, `timing`, `acknowledgement`, `drafts` or `choices` | Follow the ordinary member composer through ambiguous work requests, watch intervals, lost acknowledgements, two-tab recovery, worklog navigation and one-off choices alongside standing preferences. Each group uses its own member volumes. |
| `--preference-recall` or `--preference-recall --claude` | Stated preferences, same-thread and cross-thread advice, one-time choices, corrections, forgetting and account isolation. Both modes drive the member composer. |
| `--research-work` or `--research-journey` | Shopping, applications, actual source reads, mobile navigation and manual browser handoff with a synthetic broker |
| `--brief-work`, `--public-watch` or `--public-watch-read` | Recurring requests from chat, connection requirements, check history and stop |
| `--claude` or `--short-corrections` | Ten real Claude turns including three game follow-ups, or three synthetic correction turns |

Run each journey in its own process. `PASS` counts submitted messages, interpretations and answers for each completed case. `JOURNEY` gives the group total. Reopening a stored report is expected to add zero model calls. All groups use synthetic accounts and records on Linux. [Conversation and memory journeys](../../../docs/reviews/2026-11-25-live-member-journeys.md) · [Composer and recovery journeys](../../../docs/reviews/2026-11-25-composer-routing-and-recovery.md)

The real model runs require a logged-in first-party Claude CLI. For Gmail and Calendar the Linux Rust route suites use isolated provider doubles and check returned identifiers, current grant, final approval, cancellation and recovery. [Scenario mapping](../../../docs/reviews/2026-11-25-member-work-from-chat.md)

## Observed result, 2026-11-19 KST

| Check | Observed result |
|---|---|
| A/B identity and private storage | UIDs10001/10002; `a-private` / `b-private`; only own bind mounted |
| Own runtime Unix socket | `a:own-runtime` / `b:own-runtime` |
| Owner-scoped synthetic broker Unix socket | `a:broker-ok` / `b:broker-ok`; broker checks `SO_PEERCRED` UID |
| Other account paths, host marker and engine socket | Absent in both runtimes |
| Root filesystem and shared module write | Rejected with EROFS30 |
| Capabilities / NNP / seccomp | All capability masks zero; `NoNewPrivs:1`, `Seccomp:2` |
| Actual cgroup controls | `memory.max=67108864`, `cpu.max=50000 100000`, `pids.max=32` |
| Per-account disk cap | ENOSPC28 after about26.5MB; owner marker remains; other account unaffected |
| Daemon restart | Both active accounts resume and retain owner/start journal |
| Explicit stop | A stays stopped across daemon restart; B resumes |
| Full guest reboot | Both active accounts resume, mounts precede engine, same private markers and sockets |
| Missing filesystem | Kiri dependency start fails; unmounted fallback directory has no owner file; restore resumes |

The final reproducible profile uses `NetworkMode:none`, no published TCP ports,
no engine socket mount, no host HOME or agent mount, read-only shared software and
owner-scoped Unix sockets. It permits normal synthetic broker traffic over each
owner's dedicated socket without a container network route.

## Adoption boundary and upstream limitations

**The constrained primitive profile passes this bounded smoke; the complete
production adoption gate remains open.** It still needs the actual Rust
provisioner/gateway/model/egress brokers, user authentication and generation
revocation, owner DB/full-volume backup restoration, CPU/memory/PID overload
behavior, realistic account capacity and the rest of the OpenSpec acceptance.
These scripts do not prove adversarial escape resistance or real-provider support.

A separate ordinary synthetic observation confirmed the design's warning:
Kiri `Internal:true` removed the default route and blocked reaching a peer on a
different subnet, but an ordinary request could still reach a synthetic service
on its own bridge gateway. That configuration does not satisfy the no-host-service
contract. Its weaker-network reproducer is intentionally not part of this artifact;
retain `network:none` until explicit owner-scoped communication is implemented.

Kiri's API accepts/inspects security options it does not necessarily enforce:
the exact source implements `no-new-privileges` and `seccomp=unconfined` handling,
but no AppArmor/SELinux profile enforcement or UID namespace remapping was found.
Do not pass arbitrary `SecurityOpt`, network/mount/driver options or images from
user input. The local volume driver rejects a requested `size` quota; a filesystem
cap must be enforced outside Kiri. Named volumes are populated lazily: the first
empty-volume population can reset a precreated directory's UID to the image's root
owner. The final smoke avoids that ambiguity with explicitly owned bind mounts.

On a running container, inspect retained `State.ExitCode:137` after daemon restart
and `RestartCount:0` despite a new PID. Use current `Running`/`Status`, actual
process identity and the application's own durable journal for recovery evidence.

The published release asset inventory has lite DEB amd64/arm64, lite/full RPM
x86_64/aarch64, Mac DMGs, Windows MSIs and tarballs. The inspected GitHub release
API did not list a full DEB or `SHA256SUMS-linux`, although the live docs describe
them; this experiment pins the actual release asset SHA256 provided by GitHub.

Sources: [Linux](https://docs.runkiri.com/linux/),
[v0.2.1 assets](https://github.com/libracoder/kiri/releases/tag/v0.2.1),
[API source](https://github.com/libracoder/kiri/blob/b249eccc2ed708ce10d7feeb82b6c0869b112443/crates/kiri/src/daemon/api.rs),
[Linux container source](https://github.com/libracoder/kiri/blob/b249eccc2ed708ce10d7feeb82b6c0869b112443/crates/kiri-init/src/linux/container.rs),
[volumes](https://github.com/libracoder/kiri/blob/b249eccc2ed708ce10d7feeb82b6c0869b112443/crates/kiri/src/daemon/volumes.rs).

## Installation-wide vault handoff

Run `python3 member-vault.py /absolute/path/moosuri /absolute/path/moosuri-vault-service`
as a nonroot Linux user. The stdlib-only fixture starts real gateway, member and
vault processes with two synthetic member stores in a temporary private directory.
It checks a member added after gateway startup, one-time handoffs, both directions
of account isolation, logout, gateway restart/reopening and registry fencing.
It makes no model or Google calls. It does not validate browser recipient setup,
client encryption UI, HTTPS provisioning or the public installer.
