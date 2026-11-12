# Rendered public-page reading acceptance

The chat tool `browser_read` now calls a typed Rust controller that launches a fresh
Linux Chromium capsule. The sole model argument is a public URL. Its persisted
observations are labeled as browser pages, separately from HTTP pages and search
snippets. [Configuration and limits](../browser-chat.md) describe the operator setup.

## Implemented boundary

New tool profile4 pins the complete canonical deployment configuration. Profiles1–3
retain their original vocabulary; retry cannot acquire browser authority or replace
the deployment. Account UUID comes from the current authenticated tool permit.
The same durable slots, cumulative6-model/12-tool/120-second budget, current-authority
checks, dependency withdrawal and unknown-outcome handling apply to browser IO.

The launcher uses a fixed Docker executable/socket, immutable image ID, exact owned
network/proxy identities and reviewed seccomp bytes. A reserved empty Docker config
prevents inherited proxy settings or credentials. No home, database, credential file
or Docker socket is mounted in the worker. Chromium's sandbox remains enabled; the
worker drops every capability set after its namespace firewall is established.
The proxy validates public destinations on every request, including subresources.

The assistant cannot click, type, log in, submit forms, execute arbitrary scripts or
send mail through this tool. Worker-level screenshot/handle controls are not model
tools. Every actual Gmail send retains its exact full-preview human approval.

## Verification and defects found

Rust1.89 workspace: **270 passed** on macOS and Linux x86_64, with one opt-in live
account test ignored. Counts are CLI2, installation1, agent50, connectors99,
server2, HTTP24 and Store92; the nested process-lock child is not counted twice.
Workspace all-target clippy with warnings denied and rustfmt passed. React's
**79 behavioral tests**, typecheck, production build and generated DTO comparison
passed. Python smoke-harness unit checks passed4 cases; shell/Python syntax checks
also passed. Scripted model-loop tests are execution-boundary tests, not model quality.

Independent review covered the49-file initial integration and the later four-file
cleanup/shutdown correction. It checked identity, protocol bounds, cancellation,
tenant scoping, old-run pins, durable provenance and unchanged mail approval.
The following real Linux test failures were preserved and fixed:

- The test canary's file permissions prevented the dropped UID from reading it;
  only the test image's fixture modes were corrected.
- Chromium's sandbox required a namespaced `chroot`, but the upstream seccomp rule
  depended on an initial capability absent from this capsule. The derived profile
  changes only that rule's capability condition and explanatory comment. Upstream
  bytes and Apache attribution are retained. No host capability or sandbox-disable
  fallback was added. Direct non-namespaced `chroot` still returns `EPERM`.
- Production `docker run --rm` cleanup raced with the controller's removal request.
  Diagnostics observed successful page reads followed by “removal already in progress,”
  incorrectly yielding unavailable. Failed removal now requires bounded confirmation
  that the same previously observed full immutable ID is absent. Daemon errors,
  wrong identity or remaining resources are not treated as success. A regression
  test covers disappearance, delayed removal, persistent presence and daemon errors.
- Independent review found the outer5-second shutdown wait shorter than the new
  bounded cleanup path. It now encloses the6-second run and2-second mail drains
  with9seconds; a paused-time test checks concurrent callers beyond the old cutoff.

The final real Linux smoke passed on Linux5.15/Docker28 with Chromium156.0.8078.79
in Debian12. It exercises rendered JavaScript, visible negation, a real PNG,
cross-worker/stale handles, private subresource/redirect denial, DNS rebinding,
direct-egress denial and an owned loopback canary with a successful positive control
and zero browser hits. Effective, permitted, inheritable, ambient and bounding
capabilities were all zero, with NoNewPrivs and seccomp active. Owned containers
and networks were confirmed absent after cleanup. No host policy was modified.

After the cleanup fix, a separate direct test of the production Rust CapsuleReader
opened the real public example.com page **5/5** times and completed owned cleanup.
Observed end-to-end controller times were1.959–3.425seconds, including launch and
cleanup. This is a single-page repeated integration check, not a throughput or
research benchmark. Temporary diagnostics were confined to a separate private
source copy and are absent from the published code.

The actual ChatGPT model (`gpt-6.1-terra` / `high`) then completed the real API→agent→
production capsule→answer path using the existing official login through a private
development bridge. The persisted slot was `browser_read`, with `browser_page`
provenance, the actual title/URL and no HTTP fallback. It took two model dispatches
and8.649seconds. This includes bridge/startup overhead and is not TTFT. Login tokens
remained on the Mac; only a generated temporary bridge bearer reached the owned
Linux test directory and was removed afterward.

Failures remain part of the record: two fresh tool-verification attempts in this
browser evaluation returned no final answer through the bridge; another attempt
passed verification but hit the cleanup defect, then the deliberately bounded
evaluation call cap. The successful final trial reused that previously verified
unchanged provider revision/model, checked its stored text/tool proof through the
normal API, and created a new run after the code fix. It did not force a capability
flag or alter proof records. Fresh subscription-bridge proof reliability is still
an unresolved integration limitation, separate from the browser controller result.

## Publication and limits

The11 additional registry dependencies have declared licenses and returned no OSV
advisories; the prior267 were checked in the preceding unit. This is not a guarantee
of vulnerability absence. Reviewed upstream seccomp bytes, derived changes and
third-party notices accompany the source. Public allowlisting, secret detection,
OpenSpec and planning/link validation are separate publication gates. The311-file
public source allowlist, secret scan,19 strict OpenSpec items and planning/link
checks passed before publication; new commit history is scanned before push.

Operator `verified` and smoke hashes are assertions, not remote attestations. An
operator must validate their own exact image and host; this development host's
result does not certify another deployment. Unconfirmed delayed Docker creation
can require scoped operator reconciliation. No real Google OAuth, paid Brave
search, email send, production cutover or collector/cron/database change occurred.
CI remains disabled. Authenticated browsing and general external writes remain
future implementation.
