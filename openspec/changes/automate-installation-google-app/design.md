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
the connection systemd service launches a developer VM wrapper. The gateway's
vault adapter is also enrolled for one account. These are release-blocking
integration gaps, not prerequisites to ask a new installer to supply manually.
Until packaged and tested, retain the current support warning and do not switch
the public install command to an incomplete Kiri deployment.

The gateway owns setup state and administrator authorization. The existing Kiri
transport owns the isolated browser and secret-bearing Console responses. New
workflow decisions belong in Rust. React shows progress and recovery in the
same Google connection flow. No Cloud Shell, local Mac credential dependency,
model-visible password, raw Console trace or client-secret download is required.

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

Official references (checked 2026-11-26):
[Workspace project setup: billing is optional](https://developers.google.com/workspace/guides/create-project),
[Resource Manager: project creation does not associate billing](https://docs.cloud.google.com/resource-manager/docs/creating-managing-projects),
[SDK browser authorization](https://docs.cloud.google.com/sdk/gcloud/reference/auth/login),
[versioned SDK archives](https://docs.cloud.google.com/sdk/docs/downloads-versioned-archives),
[client creation through Console](https://developers.google.com/identity/protocols/oauth2/resources/best-practices#manual-client-creation),
[web clients](https://developers.google.com/workspace/guides/create-credentials),
[GWS](https://github.com/googleworkspace/cli).
