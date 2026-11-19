# Optional public Chromium research capsule

This is a Rust worker and network proxy for optional Linux Chromium.
[One-shot reads](../../docs/browser-chat.md) and [persistent task workspaces](../../docs/browser-workspaces.md)
share the same deployment boundary. Source checks alone do not establish containment;
the Linux smoke below must pass for the actual image and host before enabling tools or JavaScript.
No host Chrome, personal profile, credentials, app DB or Docker socket is mounted.

The backend creates a separate capsule per tenant. Typed Rust methods are
`BrowserSession::launch`, `open`, `read`, `screenshot`, `close`, and `shutdown`.
An opaque page UUID, worker generation and navigation generation bind every handle.
Cross-worker/closed handles fail; changed main-frame loader/frame identity fails.
The one-shot mode permits at most four created tabs and a 120-second lifetime.
The `--workspace` mode retains one profile per workspace, with a maximum eight-hour worker
lifetime and thirty-minute idle cleanup in the app broker. Operations, CDP messages,
text, links and PNGs remain bounded. Human frames travel only through authenticated
no-store endpoints into React memory, never model tool text or ordinary artifacts.
The app exposes `browser_read` for fresh reads and `browser_open`/`browser_observe`/
`browser_handoff` for persistent workspaces. Low-level input and screenshot APIs
remain human-only.

Kiri runs `moosuri runtime-browser-relay --registry PATH --socket-dir PATH --lifecycle-config PATH --capsule-config PATH`
on Linux. Use the same registry and lifecycle enrollment as member provisioning,
a dedicated root-owned 0700 socket root and a private JSON file containing the
verified capsule configuration. The relay discovers new member allocations every
second. It binds one UID-owned `browser.sock` inside each root-owned UUID directory.
Only this directory is mounted in its member runtime. A ready registry allocation,
matching generation, OS peer UID and current route epoch are required. Two fixed
Docker container slots per installation also count surviving workers after restart.

The member protocol opens an ephemeral profile for each task. Each profile
tracks up to four tabs, including public pages opened by the site. Switching or
closing the active tab clears its input proof. Closing the last tab shuts down
the worker. Read navigation creates a new tab for a named, currently rendered,
same-origin link observed in the current document. After the first human input
the worker accepts further model observations only through explicit frame-bound
selection and disables autonomous link navigation for that workspace.

`MOOSURI_BROWSER_JAVASCRIPT=enabled` enables actual page scripts only after the
operator has verified this deployment. The default `disabled` capability reports
that limitation. One-shot DOM reading uses fixed internal `innerText` code;
workspace observation combines Accessibility with visible DOMSnapshot text while
omitting form/editable values. Human control accepts typed click/text/key/scroll
commands, not JavaScript. There is no model-supplied evaluate, click, type, form,
upload, authenticated-profile attach or download interface. No generic browser action can send an approved mail draft;
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
one-shot reads receive no credentials. Humans may log in inside their workspace.
After takeover, the model cannot navigate that workspace; only human input can
perform generic authenticated actions. Browser cookies are server-managed and are
not a malicious-admin-resistant vault. No vault autofill is implemented.
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
cover synthetic HTTPS login/POST and secure-cookie continuity, fresh-profile
isolation, omitted form secrets, one-use frame proofs, stale geometry/overlay/focus
rejection and animated-page typing, as well as JS-rendered text, aria-hidden visible
negation, real PNGs, cross-worker and
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

The default worker reads bounded JSON lines on stdin, emits a readiness record, and accepts
`{"op":"open","url":"https://example.com/"}`, then `read`, `screenshot` or `close`
with the exact returned `handle`, and `{"op":"shutdown"}`. Stdin EOF/cancellation
shuts down Chromium. The Docker init process reaps child processes; capsule removal
discards its tmpfs profile unless the operator enables bounded workspace recovery below. The app must never automatically reattach old handles
after a worker restart or replay an ambiguous external action.

Workspace mode instead accepts `workspace_open`, `workspace_observe`,
`workspace_frame` and `workspace_input` (plus shutdown). The Rust broker keeps the
worker attached to the original user/conversation independently of app tabs or chat
streams. Replacing the image/config requires new smoke evidence; existing runs
cannot silently switch their pinned deployment. See [the API/lifecycle contract](../../docs/browser-workspaces.md).

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
sh -n deployment/browser/entrypoint.sh deployment/browser/proxy-test.sh deployment/browser/chromium-test.sh
```

These local checks do not establish actual Linux/Chromium containment. The
Docker smoke test remains a release gate for each deployment, even when the
repository records a successful synthetic test on a different image/host.


The unchanged upstream profile is retained as `seccomp-upstream.json`. The derived
`seccomp.json` adds only unconditional `chroot` syscall permission for Chromium's
zygote after entering its user namespace; the upstream CAP_SYS_CHROOT-conditional
rule prevents that sandbox startup under `cap_drop: ALL`. This grants no host
capability. Entrypoint still clears every capability set, uses UID10001 and
NoNewPrivs; production health checks require Eff/Prm/Inh/Amb/Bnd all zero and
Seccomp2. The synthetic worker additionally checks direct non-namespaced
`chroot("/")` fails EPERM. Keep those controls in actual Linux smoke evidence.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for exact hashes and attribution.

## Optional bounded workspace recovery

Add `"recover_workspaces": true` to the reviewed capsule JSON only after testing
this image/host. It defaults to false and false is omitted from canonical config
serialization, preserving existing pins. Enabling it changes the deployment pin.
The worker must explicitly report `profile_persistence: true`; older images fail
closed. One-shot public reads and workspaces with the option disabled retain
tmpfs profiles and their existing discard behavior.

The app creates dedicated local Docker volumes with exact owner/project/account/
workspace/generation labels, mounts only their fixed profile directory and never
mounts a personal host browser. Only the original authenticated app session can
request recovery before the original idle/absolute deadlines. Human recovery
starts a fresh blank target; it does not restore tabs, navigation, POSTs, input or
service workers. Persistent cookies/localStorage committed by Chromium may remain;
session cookies may expire and sites may ask the user to log in again. Cookies
remain server-managed, with no vault or malicious-admin protection.

Close/expiry/deleted thread/account/revoked session and deployment-pin changes
invalidate the lease. Startup/background reconciliation removes only exact-owned
ineligible profiles and stale workers. Failed Docker cleanup leaves the labels
and invalid DB state for retry; it never restores authority. If this app/daemon
stays stopped, cleanup is delayed. Retiring an owner/project requires separately
scoped operator cleanup. Never prune volumes across tenants or mount arbitrary
host storage as a substitute.

Local Docker volumes do **not** impose a hard per-profile filesystem quota.
Provision disk capacity, quota/monitoring and operational cleanup before opting
in. Downloads remain denied; small disk/media cache limits reduce routine cache
use but do not cap cookies/site storage. Do not describe this option as indefinite
or guaranteed crash-durable login retention. The synthetic smoke now exercises
persistent-cookie continuity across worker restart and requires a blank recovery,
no prior GET/POST replay and invalid old frame tokens.
