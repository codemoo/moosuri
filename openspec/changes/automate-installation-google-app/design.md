# Design

Reuse the installation registry's Google web-client record, administrator role
checks, atomic registration and existing per-member GWS distribution. Do not add
a second credentials store or copy an administrator's refresh token to members.

The acceptance run SHALL create a fresh project and a fresh OAuth client from
a public installation with no Google/GWS configuration. Project creation and
the requested Workspace APIs do not imply a billing prerequisite. Inspect the
actual project form first and prefer an explicitly unbilled project. Record
unchecked, not-required, required-with-choices, required-without-an-account and
explicitly-selected billing states separately. Never choose an account from a
recent project, identical display names, the sole available account or elapsed
time. Do not create paid resources or a billing account as a setup side effect.

Google first-use terms, organization restrictions, quotas, identity verification
and application verification are provider boundaries. Show the specific required
action in the existing connection flow and resume the same setup afterward.
Do not substitute Cloud Shell, a developer's credentials or an unrelated project.

Project and API preparation can use the official Google Cloud CLI installed
inside the isolated Kiri setup runtime. Its initial SDK authorization runs in
that same Chrome profile, with a new private configuration and no inherited
Cloud credentials. This avoids needing the installation's own OAuth client in
order to create its first project. Keep CLI credentials within the temporary
runtime and expose only bounded setup results to Rust. Google Console still
owns the OAuth client configuration. This route was verified for a fresh,
unbilled project and five Workspace APIs; it is not yet product integration.

## Installation boundary

The installer must provision the member runtime, browser transport, vault origin
and recipient enrollment, and GWS distribution from versioned public artifacts.
Use the existing Rust lifecycle/onboarding and member allocation machinery;
do not add a second installer backend or copy a private skill into the product.
Bind browser transport and recipient keys to the authenticated account,
generation and volume. A fixed `owner.sock` or a host-specific SSH wrapper is
not a portable member routing scheme. A shared OAuth client is installation
configuration; browser sessions, vault keys and Google tokens are member data.

The current public `install.sh` runs the standard server, while Kiri is manually
assembled. The guest imports `fresh_browser`/`secure_google` from a private skill;
the connection systemd service launches a developer VM wrapper. The gateway now
supports installation-wide vault enrollment, with member-scoped token routing
and lifecycle delivery. Production retains fixed-member mode until its Google
browser/recipient transport supports the same allocations. These are release-blocking
integration gaps, not prerequisites to ask a new installer to supply manually.
Until packaged and tested, retain the current support warning and do not switch
the public install command to an incomplete Kiri deployment.

The gateway owns setup state and administrator authorization. The existing Kiri
transport owns the isolated browser and secret-bearing Console responses. New
workflow decisions belong in Rust. React shows progress and recovery in the
same Google connection flow. No Cloud Shell, local Mac credential dependency,
model-visible password, raw Console trace or client-secret download is required.

### Portable authentication worker

The public Rust worker runs inside a fresh Kiri capsule. It prepares a one-use
RSA-3072 recipient before starting Chrome; only the public SPKI leaves the
capsule. Retain the vault's RSA-OAEP-SHA256/AES-256-GCM envelope, with a new
versioned binding covering installation, account, tenant, volume, generation,
authorization epoch, challenge, member email, selected Google email and callback
origin. The private key exists only in the capsule's private tmpfs and is removed
when the attempt exits. OpenSSL supplies RSA operations; ring supplies AES-GCM.
No host key enrollment or private Python module is a runtime dependency.

An authenticated gateway reserves the attempt, transports its public recipient
to the vault, then forwards the vault ciphertext. Successful decryption is not
authorization to open Chrome: the worker waits for a separate release after the
gateway rechecks the exact member/session/allocation. Connection loss, malformed
messages or deadline expiry end the worker. Only enumerated phase receipts leave
it; CDP, provider page contents, passwords and cookies remain inside.

The official Chrome build runs with its sandbox and a private X display; it is
separate from the public research browser protocol. The same ephemeral profile
must serve account authentication, SDK setup, Console setup and member consent.
There is no general script or browser-input method on the authentication wire.
Provider rejection is terminal, and a device challenge remains a visible wait.

Before external writes, recheck the initiating administrator and installation
identity. Bind the client to the server's HTTPS callback. Persist provider
resource identifiers, pre-write intentions and checkpoints without secrets so a lost response does
not create another project or client. Reject replacement of an existing app;
restart continues from the last confirmed resource. Cancel closes the temporary
browser and retains confirmed resources and uncertain intentions for reconciliation.

Registry migration 8 introduces one nonsecret installation checkpoint and a
renewable lease. One private run identifier belongs to one worker; duplicate
claims are refused. Resume keeps the original project identity and selected
Google account. Every action rechecks the administrator's current private
session, generation, role revision and epoch. Lease expiry is not permission
for an older worker to write. All registry-reading services must be upgraded
together before opening a migrated production registry.

Application reuse and user consent are independent. Testing audience membership
must be explicit in status; registering an app alone does not grant access to
another Google account or make it production verified.

Use External audience for an installation shared by independent Google accounts.
Personal mode cannot satisfy that contract: Google limits it to the creator and
does not allow later conversion to External or Internal. An existing application
must never be silently converted or replaced to avoid an audience restriction.

Capture the new web client's secret into the private registration path at creation.
Google only reveals its full value once. After an interrupted creation, reconcile
the stable client name, project and exact callback, then adopt an already committed
private file when present. If the secret was lost before local commit, use the
same client's supported secret-rotation flow; do not pretend a masked value is
recoverable or create another client. Rotation itself needs a pre-write intent,
bounded reconciliation and explicit handling of Google's two-secret limit. Until
that recovery is implemented and tested, the client-creation gate stays open.

These provider requirements were rechecked on 2026-11-27 in
[Google Auth Platform onboarding](https://support.google.com/cloud/answer/15544987?hl=en)
and [client and secret management](https://support.google.com/cloud/answer/15549257?hl=en).

Official references (checked 2026-11-26):
[Workspace project setup: billing is optional](https://developers.google.com/workspace/guides/create-project),
[Resource Manager: project creation does not associate billing](https://docs.cloud.google.com/resource-manager/docs/creating-managing-projects),
[SDK browser authorization](https://docs.cloud.google.com/sdk/gcloud/reference/auth/login),
[versioned SDK archives](https://docs.cloud.google.com/sdk/docs/downloads-versioned-archives),
[client creation through Console](https://developers.google.com/identity/protocols/oauth2/resources/best-practices#manual-client-creation),
[web clients](https://developers.google.com/workspace/guides/create-credentials),
[GWS](https://github.com/googleworkspace/cli).
