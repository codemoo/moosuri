# Optional rendered public-page reading in chat

`browser_read` lets the ordinary assistant read one public URL using a fresh Linux
Chromium capsule. Its sole argument is `url`. It returns actual rendered text,
title, public links, retrieval time and explicit JavaScript mode. The run's
recorded observations label these as browser pages, separately from HTTP pages
and search snippets. Public content is untrusted evidence. Truncated content is
incomplete; neither the tool nor its source list proves an entire answer.

This capability is off by default. `web_read` remains the lighter HTTP reader;
profiles 1–3 retain their existing tool vocabulary. A configured deployment admits
profile 4 and pins the SHA-256 of its complete canonical configuration. Existing
runs cannot gain the capability, change deployments or regain a disabled
capability through retry. Other recipe and Gmail pins are preserved. No tool can
approve or send mail; the existing immutable preview and final human approval
still govern every send.

## Deployment contract

Use the reviewed [capsule deployment](../deployment/browser/README.md) on a
supported Linux host. Build the production image and retain successful containment
smoke evidence for that exact build and host. The controller uses the local Docker
daemon through `/usr/bin/docker` and `unix:///var/run/docker.sock`; it therefore
needs Docker authority on the host. The model receives none of that authority.
The reserved `/run/moosuri-browser-empty-docker-config` path must be absent or an
empty owner/root-controlled directory without group/world write permission. The
launcher rejects populated/symlinked configuration so Docker cannot inject stored
credentials or proxy environment variables.
No socket, home directory, database, credential file or user browser profile is
mounted inside the capsule.

An operator must provision the production proxy and a non-overlapping internal
browser network before starting the application. `compose.yaml` supplies those
resources; use a unique Compose project and a UUID string (with hyphens) as its
`MOOSURI_BROWSER_OWNER` label. Set the reviewed subnet and private proxy address,
and start only the proxy service. The application launches each capsule from the running production proxy
image, which contains both proxy and worker binaries and the default capsule
entrypoint. Derive `image_id` from that container, rather than from the separately
built manual Compose `browser` service:

```sh
# Use the exact production project; inspect only public resource identity fields.
proxy_lookup="$(docker compose -p "$BROWSER_PROJECT" -f deployment/browser/compose.yaml ps -q proxy)"
proxy_id="$(docker container inspect --format '{{.Id}}' "$proxy_lookup")"
image_id="$(docker container inspect --format '{{.Image}}' "$proxy_id")"
network_id="$(docker network inspect --format '{{.Id}}' "${BROWSER_PROJECT}_browser")"
```

Use those exact `proxy_id`, `image_id` and `network_id` values in the JSON below.
`image_id` is a local `sha256:<64 lowercase hex>` immutable image ID. Record the
full network ID, full proxy container ID, exact owner/project labels and proxy IP.
Do not use the synthetic smoke image or test proxy in a real deployment. The app
does not provision networks, start a proxy, change host firewall policy or select
images automatically.

Create an owner-only JSON regular file without symlinks or extra hard links,
maximum 16 KiB, with these fields. The repeated hex values below are placeholders;
replace each with the actual ID or evidence hash. Use an absolute path to the
reviewed `deployment/browser/seccomp.json` file, kept owner/root-owned and without
group/world write permission.

```json
{
  "image_id": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "network_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "proxy_container_id": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "proxy_ip": "10.42.17.2",
  "owner": "00000000-0000-4000-8000-000000000001",
  "project": "moosuri-browser-production",
  "seccomp_file": "/opt/moosuri/deployment/browser/seccomp.json",
  "verified": true,
  "smoke_evidence_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
  "javascript_enabled": false
}
```

Add `--browser-capsule-file /absolute/private/browser.json` to `moosuri serve`.
JavaScript requires explicit operator configuration after verification. `verified`
and the evidence hash are operator assertions, not isolation attestations; retain
the actual smoke output and build/host identity for review. Before every read the
controller checks the reviewed seccomp bytes, exact internal network ownership,
and the running proxy's exact ID, image, labels, network attachment and address.
These checks supplement the deployment smoke evidence. No browser is launched
on unsupported hosts, absent resources or failed checks.

## Execution and recovery

Each read gets a fresh capsule with the reviewed firewall, Chromium sandbox,
read-only filesystem, ephemeral profiles, dropped capabilities after entrypoint
setup and resource limits. Only validated public HTTP(S) URLs are admitted. The
proxy resolves and checks every destination and subresource; the capsule firewall
permits only the configured proxy, blocking direct network bypass. No private-IP,
metadata, localhost or host-browser fallback is available.

At most two reads execute concurrently per controller; excess reads fail
immediately. Protocol IO has a 40-second deadline, bounded readiness/response
frames, 8 KiB text, four links and 512-byte titles. The existing run-wide ceiling
remains six model dispatches, twelve tools and 120 seconds. No clicks, forms,
login, credentials, attachments, arbitrary JavaScript, screenshot or send API is
exposed to the assistant. Public page scripts may themselves initiate network
requests when enabled; the public proxy policy still applies.

The authenticated account UUID comes from the external-tool permit. Before and
during IO and again at completion, the store checks authority, admitted deployment
pin, budget and current configuration. No SQLite actor lock spans browser IO.
Cancellation or lost authority cancels IO and waits for bounded subprocess and
owned-container cleanup, even when stdout stalls. Dropping the caller also
requests independently owned cleanup. Shutdown grants a six-second task drain and a two-second mail-worker drain;
the outer nine-second wait encloses both, including concurrent shutdown callers.
A cleanup failure yields an unavailable result; it does not claim success or that
all resources disappeared. Three bounded exact-name queries reconcile possible
delayed daemon creation, and only matching owner/read labels authorize removal
by the full immutable container ID. If Docker automatic removal races with that
request, three bounded queries of the same immutable ID must confirm absence
before accepting cleanup; daemon errors or a still-present container remain failures.
If readiness was never confirmed and no
container was observed, cleanup returns `CleanupUnconfirmed`, even after empty
queries. A daemon may complete creation after this bound; known stale resources
require separately scoped operator reconciliation. Daemon failure is not treated
as proof of absence.

Completed slots replay their saved result. Interrupted dispatched slots recover
as `outcome_unknown`, without launching another capsule. Retry does not replenish
budgets or reset deadlines. Only completed, matching typed browser slots can add
browser observations; model prose and HTTP result payloads cannot manufacture
that provenance. Existing tenant scoping and source/thread withdrawal apply.

## Verification limits

Local tests use a fake worker subprocess for protocol limits, output validation,
stalled-stdout cancellation, caller drop and capacity, plus a synthetic model
server through the real runtime, store permits and evidence DTO. They do not
establish host containment. The separate Linux smoke exercises real Chromium,
JavaScript, private-resource denial, direct-loopback positive-control probes,
rebinding and cleanup. Retain its real outcome separately; source-only tests do
not establish that a production deployment is verified.
