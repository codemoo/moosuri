# Configured Linux member restart recovery

`moosuri kiri-lifecycle --experimental --config /etc/moosuri/kiri-members.json --state-directory /var/lib/moosuri-kiri-lifecycle` restores already initialized, explicitly configured members after host/Kiri restart. It reuses the exact guarded provisioner and checks owner-specific `/health/ready` headers. `--once` performs one pass; otherwise it reloads the private config every five seconds and retries each member independently. Changes are emitted as metadata-only JSON events; private `status.json` records the latest completed pass.

The operator must prepare the [existing installation layout](../README.md), local image, persistent account/IPC mounts and active registry allocations first. This command never reserves identities, initializes passwords, activates a reserved/fenced route, pulls images, creates mounts or replaces conflicting containers. It opens no legacy assistant DB. Configured account UUID, email and generation must all match the authoritative active registry record. Duplicate account, email or UID entries reject the complete config before a recovery pass.

```json
{
  "version": 1,
  "registry_directory": "/var/lib/moosuri-control/registry",
  "mounts_root": "/srv/moosuri-members",
  "ipc_root": "/var/lib/moosuri-control/member-ipc",
  "allocations_root": "/var/lib/moosuri-control/host-allocations",
  "image": "sha256:REPLACE_WITH_VERIFIED_LOCAL_CONFIG_ID",
  "memory_bytes": 536870912,
  "nano_cpus": 1000000000,
  "pids": 128,
  "max_volume_bytes": 10737418240,
  "members": [
    {"account_id": "ALLOCATED_ACCOUNT_UUID", "generation": 1,
     "email": "member@example.invalid", "uid": 10001}
  ]
}
```

The config must be an existing canonical absolute, root-owned regular file with no group/other permissions or hard links, under root-controlled ancestors. Keep it outside personal data and source control; it contains member metadata, never credentials. Update it with a root-owned atomic replacement. `model_sockets_root` is optional; per-member `enable_text_preview` defaults false and requires that broker root. For the [private Google inbox](../google.md), set `google_sockets_root` and enable `enable_google_readonly` only on the corresponding member. The state directory must be a separate existing root-owned 0700 directory; its directory lock prevents duplicate workers.

The included systemd unit is an opt-in template. Installing this source does not enable it. The operator selects an explicit trusted executable/config and prepares persistent filesystem mounts before copying/enabling the unit. It deliberately does not make one member mount a dependency of the whole worker: an unavailable A volume must not prevent B from recovering. The existing provisioner rejects an absent account/IPC mount and retries only after the operator's mount manager restores it. The worker leaves registry routing states unchanged and does not carry any password or session in its config/status.

A listed active member is desired running: stopping only its container is temporary while this worker is enabled. Remove it from the allowlist (or stop the worker) before an intentional long stop. Removal stops automatic management; it does not drain/stop an already-running member. Fencing continues to block the gateway/model broker, and offline fenced accounts are never restarted. Stopping already-running fenced work and complete invitation/migration orchestration remain separate contracts. A concurrent fence after a start can leave that private process running; the worker reports it inactive and never reactivates its route.

This is restart recovery for the bounded personal-service preview, not completion of full assistant workers, invite UI, storage management, quota capacity or production migration. The worker never sends an invitation or accesses a real provider.

## Verification boundary

An isolated Ubuntu 24.04 x86_64/Kiri 0.2.1 KVM guest with two vCPUs and about 2 GiB RAM restored two preinitialized members through the enabled systemd template after a real guest reboot. Their pre-reboot cookies stayed valid only on the original owner. A stale configured generation, duplicate worker and offline fenced member were rejected; loss of A's mounts left B ready with the same PID. A root fixture chowned its socket file to the expected UID but had the wrong kernel peer UID: the worker rejected it before sending HTTP bytes. Status contained neither test cookies nor passwords. Three focused Rust tests and the actual Linux build passed.

The two member limits were 256 MiB memory, 0.5 CPU, 128 PIDs and separate 64 MiB capped filesystems. One CLI recovery pass starting stopped A while checking running B took 262.455 ms. Ten sequential warm passes had median 30.567 ms and nearest-rank p95 36.957 ms; each includes CLI startup, registry/profile validation, health checks and durable status. Twenty sequential private Unix readiness GETs per member had medians 0.322/0.218 ms and nearest-rank p95 0.740/0.274 ms. These guest-local samples exclude TLS, gateway and providers; host load was uncontrolled. They do not establish capacity, a paired dev.30 comparison, or the planned baseline for 1, 5 and 20 members.

[Recorded timings, measurement definitions and artifact fingerprints](measurements.json)
