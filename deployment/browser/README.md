# Optional public Chromium research capsule

This is a runnable worker and network proxy. Optional ordinary-chat integration is
documented in [browser-chat.md](../../docs/browser-chat.md); neither is a claim that
Linux containment has passed. The authoring machine compiled and tested the Rust code;
the Linux smoke test below must pass before enabling browser tools or JavaScript.
No host Chrome, personal profile, credentials, app DB or Docker socket is mounted.

The backend creates a separate capsule per tenant. Typed Rust methods are
`BrowserSession::launch`, `open`, `read`, `screenshot`, `close`, and `shutdown`.
An opaque page UUID, worker generation and navigation generation bind every handle.
Cross-worker/closed handles fail; changed main-frame loader/frame identity fails.
There are at most four created tabs, a 120-second worker lifetime, 15-second
operations, bounded 4 MiB CDP messages, 8 KiB DOM text, four public links and 2 MiB viewport PNGs.
PNG bytes are intended for tenant artifact storage, never inline model tool text.
The optional chat adapter exposes only one fresh `open` read and uses durable
external-tool receipts; low-level screenshot/handle APIs are not model tools.

`MOOSURI_BROWSER_JAVASCRIPT=enabled` enables actual page scripts only after the
operator has verified this deployment. The default `disabled` capability reports
that limitation. DOM reading uses fixed internal `innerText` code. There is no
model-supplied evaluate, click, type, form, upload, authenticated-profile attach or
download interface. No generic browser action can send an approved mail draft;
mail remains a separate typed broker requiring final explicit user approval.

Production prerequisites: Linux Docker Engine/Compose, iptables/ip6tables in the
container namespace, unprivileged Chromium user namespaces supported by the host
kernel/AppArmor policy, and permission to build the optional Chromium image.
The entrypoint installs default-deny IPv4/IPv6 firewall rules, allows loopback
control traffic inside the capsule and TCP only to the egress proxy, then drops
all capabilities, UID/GID and privilege escalation. Failure is fatal; there is no
`--no-sandbox`, privileged-container or host-network fallback. Host kernel policy
must be configured by the operator; do not weaken it automatically.

The browser has only a private Docker network. A separate unprivileged proxy has
external connectivity. HTTP/CONNECT targets are parsed and every DNS answer must
be public; sockets connect to vetted addresses without a second DNS lookup.
HTTP is restricted to GET/HEAD port80, CONNECT to port443. Redirects, frames,
scripts and service-worker requests pass through the same proxy. The firewall
blocks direct TCP/UDP, QUIC, WebRTC/DNS and private-network bypass. Proxy alone
without the firewall is insufficient. CONNECT does not inspect HTTPS methods;
this capsule deliberately receives no personal credentials or arbitrary actions.
Namespace-local loopback serves the worker's CDP control plane; browser page
traffic to loopback must still pass through the proxy and be denied. Chrome's
implicit localhost proxy bypass is disabled. The health marker records
successful entrypoint setup; it is not cryptographic host attestation.

The image explicitly installs distro Chromium and libraries; all authored runtime
logic is Rust. Debian tags/package repositories are mutable: record the built
image digest and `chromium --version` in deployment verification, maintain Chromium
security updates, and pin the resulting image digest in an actual deployment.
No Node/Python service is required by the production image. Python exists only in
the synthetic test image/host test harness. Operator capacity policy must bound
the total number of tenant capsules; this prototype does not create a fleet.

From the repository root on a **disposable Linux test host**:

```sh
python3 deployment/browser/smoke.py
```

Every invocation chooses a UUID project/owner label and checks existing Docker
subnets plus host/VPN routes before creating its network. To request a specific
unused RFC1918 /24 through /28 subnet, set `MOOSURI_BROWSER_SUBNET`; the harness
derives its second host as `MOOSURI_BROWSER_PROXY_IP` and rejects mismatched
inherited values. A collision aborts before creation. Container/network ownership
labels are checked before exec and cleanup. A failed worker stop or Compose down
does not skip remaining individually verified owned resources; no fixed project,
`--remove-orphans`, host firewall update or host-network fallback is used.

The harness bounds worker readiness, commands, response bytes, process shutdown,
and Docker command deadlines. It builds the image and starts an isolated test proxy. A synthetic public IP is
assigned only to that proxy container's loopback, so the actual production IP
policy and pinned TCP path run unchanged without public web requests. Assertions
cover JS-rendered text, aria-hidden visible negation, real PNGs, cross-worker and
closed handles, stale navigation, private redirect/image/frame/fetch and DNS-rebinding canaries, plus
direct egress denial. Private fixture listeners have successful HTTP positive
controls before deny assertions. A test-only live listener inside **each browser
namespace** is positively checked before Chromium launch; zero later loopback
hits are required. Rebinding uses fresh workers below the tab budget and requires
an increased private DNS query counter, so a tab-limit error cannot count as a
network-policy success. Direct TCP probes target only owned fixture listeners,
never the host gateway, metadata services or unrelated public hosts. The test requires Docker image/package downloads during
build. Its proxy fixture adds a capability only in `compose.test.yaml`; never
use that override or fixture in production. Failed assertions are release blockers.

For an operator-reviewed deployment after the smoke test passes:

```sh
# Choose an unused subnet after checking Docker networks and host/VPN routes.
export MOOSURI_BROWSER_SUBNET=10.42.17.0/24
export MOOSURI_BROWSER_PROXY_IP=10.42.17.2
export MOOSURI_BROWSER_TENANT=00000000-0000-4000-8000-000000000001
export MOOSURI_BROWSER_OWNER="$(cat /proc/sys/kernel/random/uuid)"
export MOOSURI_BROWSER_JAVASCRIPT=enabled
# Use a unique project for this tenant capsule; retain its name for cleanup.
export BROWSER_PROJECT="moosuri-browser-$(cat /proc/sys/kernel/random/uuid)"
docker compose -p "$BROWSER_PROJECT" -f deployment/browser/compose.yaml build
docker compose -p "$BROWSER_PROJECT" -f deployment/browser/compose.yaml up -d proxy
docker compose -p "$BROWSER_PROJECT" -f deployment/browser/compose.yaml run --rm -T --no-deps browser
```

For ordinary chat, keep the production proxy running and follow
[the trusted JSON configuration steps](../../docs/browser-chat.md). Set the app
`image_id` to `docker container inspect --format '{{.Image}}'` of that exact
running production proxy: its image contains both binaries and the default
capsule entrypoint. The app launches that immutable image directly; it does not
use the separate manual Compose browser service image.

The proxy IP is required, canonical RFC1918 IPv4 with fixed TCP port3128.
Entrypoint validation derives the firewall destination and the root-owned health
marker from that same value. Rust health checks require an exact marker/config
match; configuration changes cannot silently reuse a differently configured
firewall. Docker requires the static proxy address to belong to the configured
subnet. The smoke test additionally rejects pre-existing subnet/route overlap.
Production operators must perform that inventory for their chosen deployment.

The worker reads bounded JSON lines on stdin, emits a readiness record, and accepts
`{"op":"open","url":"https://example.com/"}`, then `read`, `screenshot` or `close`
with the exact returned `handle`, and `{"op":"shutdown"}`. Stdin EOF/cancellation
shuts down Chromium. The Docker init process reaps child processes; capsule removal
discards its tmpfs profile. The app must never automatically reattach old handles
after a worker restart or replay an ambiguous external action.

The seccomp profile derives from Microsoft's Apache-2.0 Playwright
v1.56.1 Docker profile:
https://github.com/microsoft/playwright/blob/v1.56.1/utils/docker/seccomp_profile.json
Upstream SHA-256: `cc3e61cabda6bbc1e53e54d27ba4d55a9d3be829b6dd1a596f4a7b31b1cc7849`.
Derived SHA-256: `18c6523f4de6aebb9d220d98fea8a03edb381d97237f954ed9d1f47d233f661c`.
It extends the Docker default profile to permit Chromium user namespaces. Its
compatibility with the actual distro Chromium/kernel is part of the Linux gate.
The complete upstream Apache-2.0 license and attribution accompany the profile in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Local authoring checks (no Docker or Linux browser launch):

```sh
cargo test --locked -p moosuri-connectors browser::
python3 -m unittest discover -s deployment/browser -p test_smoke.py
python3 -m py_compile deployment/browser/*.py
sh -n deployment/browser/entrypoint.sh deployment/browser/proxy-test.sh
```

These local checks do not establish actual Linux/Chromium containment. The
Docker smoke test remains an unperformed release gate until the operator runs it.


The unchanged upstream profile is retained as `seccomp-upstream.json`. The derived
`seccomp.json` adds only unconditional `chroot` syscall permission for Chromium's
zygote after entering its user namespace; the upstream CAP_SYS_CHROOT-conditional
rule prevents that sandbox startup under `cap_drop: ALL`. This grants no host
capability. Entrypoint still clears every capability set, uses UID10001 and
NoNewPrivs; production health checks require Eff/Prm/Inh/Amb/Bnd all zero and
Seccomp2. The synthetic worker additionally checks direct non-namespaced
`chroot("/")` fails EPERM. Keep those controls in actual Linux smoke evidence.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for exact hashes and attribution.
