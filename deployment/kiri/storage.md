# Explicit Linux Kiri storage preparation

`kiri-storage` prepares a fresh per-member bounded filesystem for the experimental
Kiri path. It is an explicit Linux root command; it is not the default installer,
account migration, arbitrary disk formatter, resize tool, or cleanup command.
Existing installations and unrelated disks/mounts are not adopted.

## Operator sequence

Use a dedicated initially empty root-owned 0700 storage directory below root-owned,
non-writable, traversable ancestors, for example `/var/lib/moosuri-kiri-storage`.
The separate installation registry must already contain the invitation reservation.
Keep the original request UUID, normalized email, and explicit dedicated UID.
The UID/GID must be in 10000..60000, absent from host passwd/group, and unique across
this storage root. The installer must also allocate disjoint UIDs across any other
installation roots on the same host; automatic host UID allocation is not included.

```sh
sudo install -d -m 0700 /var/lib/moosuri-kiri-storage
sudo moosuri kiri-storage --experimental \
  --registry-directory /var/lib/moosuri-account-registry \
  --request-id ORIGINAL_INVITATION_UUID --email member@example.test \
  --root /var/lib/moosuri-kiri-storage --uid DEDICATED_UID \
  --capacity-bytes 10737418240
```

Capacity is fixed backing bytes including filesystem overhead, between 64 MiB and
1 TiB, divisible by 4096. The host must have that much physical capacity available;
sparse overcommit is refused. Formatting explicitly selects ext4 and disables
mkfs discard. A post-format allocation verifies the full reservation remains.
Private database/WAL/journals, scratch files, attachments and IPC all share the
same bounded filesystem. The usable payload capacity is smaller than backing size.

The JSON result identifies the allocation/generation/UID/capacity and paths:
`mounts_root`, `ipc_root`, `allocations_root`, `member_data`, `scratch`, `ipc`, plus
the exact filesystem and IPC `.mount` unit names. Pass these roots to `kiri-invite`
or the installation's explicitly configured onboarding path. This command never
initializes a password/database, starts a member runtime, or enables routing.

The owned storage root becomes 0711 so dedicated UIDs can traverse known paths.
`mounts` is explicitly 0755 even under umask 0077. Its mounted filesystem root is
root-owned; `member-data`, `scratch` and `ipc` are member UID/GID, mode 0700. `images`,
`records`, `assignments` and the host `ipc` parent remain root-owned 0700. A dropped
UID child checks actual mount-path traversal, including ACL effects, before any
formatting and again for member-data. The host IPC path is opened/bound by root;
inside Kiri the member sees its own mounted `/ipc` directly.

## Replay, authority and failure boundaries

The public preparation function performs its own registry checks: same normalized
email and exact account/tenant/volume/generation, original invitation request, and
reserved or ready unsuspended state. A ready allocation may only reconcile an
already-ready storage record. Fenced/suspended generations are refused; this
command never unfences or resumes them. A concurrent fence can leave newly
prepared storage on disk, but final authority recheck refuses a successful result.

A durable record binds root, request, allocation/generation, UID, cap and backing
file device/inode. Exact retries reconcile the existing resources. Changed UID,
size, generation, backing inode, existing populated targets or foreign units are
refused. No user-supplied source path is accepted. New images use `create_new`;
unknown files are never overwritten. A formatting-dispatched record is inspected
for the expected ext4 UUID instead of running the formatter again.

A crash leaving an unclaimed image, incomplete filesystem, or `.next` record
requires operator inspection. The command preserves these artifacts and stops;
it does not guess whether they are disposable or silently roll back. Root-owned
image replacement, hole punching, permission changes, or snapshot restoration may
invalidate the record and require separate recovery. Automatic migration, restore,
shrink/grow and destructive recovery are outside this path.

## Reboot activation and retirement

Only exact generated units are created under `/etc/systemd/system`. Existing
conflicting files/symlinks are refused. The filesystem unit mounts the owned image
with `loop,rw,nosuid,nodev,noexec` and `RequiresMountsFor` its image parent. The IPC
bind unit has `Requires` and `After` the filesystem unit. Both exact links are added
to `local-fs.target.wants` only after layout and mounts are verified. No fstab edit
or generic host storage service is performed. Kiri lifecycle recovery must still
verify dedicated mount identity/cap/ownership before starting a configured member;
a missing unit/mount is never permission to use the underlying empty directory.

For a stopped member, the exact returned units can be inspected with
`systemctl status UNIT` or stopped in IPC-then-filesystem order. Stopping a mount
requires first stopping that member runtime and gateway admission. Retirement is
an explicit operator operation: fence the account, stop its runtime, stop and
remove only the exact owned unit/enable links after inspecting their contents,
then run `systemctl daemon-reload`. Preserve backing image and record until the
owner's retention/recovery requirements are met. This CLI has no delete, lazy
unmount, force unmount, loop-device scan, or automatic cleanup operation.

## Validation scope

Local tests cover generated mount names/dependencies, security flag parsing,
stacked mount rejection, exact plans/results and an inert non-Linux entrypoint.
A disposable Ubuntu 24.04 x86_64 Kiri VM verified actual UID initialization under umask 0077, bounded A/B filesystems,
retry/data preservation, ENOSPC isolation, foreign-source refusal, systemd mount
recovery, web invitation fulfillment and member session recovery. A guest reboot
restored all four mount units for two accounts, with the original data, bind
sources and physical disk reservations. [Recorded observations](linux-vm-smoke/onboarding-evidence.json) No production host or
native macOS mount/formatter was used. Systemd/e2fsprogs/coreutils, root access,
loop support and physical backing space are required; other Linux distributions,
architectures, SELinux policies, installation restore and concurrent host storage
administration remain unverified.
