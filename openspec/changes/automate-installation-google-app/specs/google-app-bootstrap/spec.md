## ADDED Requirements

### Requirement: A public installation prepares its own dependencies
The system SHALL support a new installer on clean Linux without existing Cloud
resources, billing, GWS configuration, private browser skills, developer files
or fixed owner-specific routing. Installation SHALL prepare member runtime,
vault recipient enrollment and browser transport from public project artifacts.

#### Scenario: A person installs the public project
- **WHEN** a new administrator installs on clean Linux with no private developer files
- **THEN** the product prepares its own Kiri and GWS runtime and starts Google connection from an empty member vault

#### Scenario: A second member connects
- **WHEN** an invited member first opens their vault and Google connection
- **THEN** their browser and recipient keys are provisioned for their own allocation without manual owner enrollment

#### Scenario: An installation is retried
- **WHEN** installation stops after allocating member storage or service configuration
- **THEN** retry reconciles the same installation and preserves its keys, accounts and completed resources

### Requirement: A prepared recipient belongs to a current member attempt
The system SHALL bind each ephemeral recipient to the installation, member
allocation, authorization epoch, challenge and callback origin. A new member's
initial epoch zero SHALL be valid. Only a current authenticated vault grant for
that allocation SHALL retrieve the key; key preparation SHALL NOT start Chrome.

#### Scenario: A member cancels before delivering a password
- **WHEN** the member cancels a prepared key and retries
- **THEN** the old recipient becomes unusable and a bounded new attempt receives a different key without blocking another member

#### Scenario: An idle prepared attempt loses its authorization or broker
- **WHEN** the member logs out, the attempt expires, or its broker exits before delivery
- **THEN** the gateway invalidates the recipient and the runtime removes the owned capsule without submitting a password

#### Scenario: A member repeatedly requests new preparations
- **WHEN** one member has a preparing or prepared key or exceeds the per-minute start budget
- **THEN** another new preparation is refused while exact request replay and a different member's valid attempt remain isolated

### Requirement: A member can recover the same pending Google connection
The member UI SHALL retain only bounded non-secret pending identifiers scoped
to the account, initiating login and service. Reentry SHALL first query the same
server-owned job and SHALL NOT issue another Start, retain credentials or infer
success from local state. Completed receipt metadata SHALL be read from the
server and pending state cleared only after confirmed completion or cancellation.

#### Scenario: The member reloads during Vault selection
- **WHEN** the same initiating login reloads before sealed delivery
- **THEN** the UI offers resume or cancellation of the existing job and uses its current challenge without reserving another key

#### Scenario: Start, delivery or completion has an uncertain response
- **WHEN** the client loses a response and reopens the connection
- **THEN** it reconciles the same job before another action, never silently restarts authentication, and preserves recoverable state until confirmation

#### Scenario: Another login or a late callback sees saved metadata
- **WHEN** the initiating login/account/service differs or an old callback arrives after a new pending job
- **THEN** it cannot adopt that work or clear the newer pending state, and the server continues enforcing current ownership

#### Scenario: A connection originated from a conversation
- **WHEN** recovery finishes outside the original thread/request
- **THEN** it does not read mail or insert the result into another conversation

### Requirement: Billing is not an assumed setup dependency
The system SHALL inspect actual provider prerequisites and prefer a project
without billing. Billing SHALL NOT be selected implicitly, and zero billing
accounts SHALL NOT alone prevent Workspace setup.

#### Scenario: No billing account exists and billing is optional
- **WHEN** Google permits project creation without billing
- **THEN** setup proceeds without asking for or creating a billing account

#### Scenario: Google requires billing for this account
- **WHEN** the provider requires billing and no selectable account exists
- **THEN** setup identifies that specific provider prerequisite and does not report a connected account or create a paid resource

#### Scenario: Several billing accounts have the same name
- **WHEN** billing is required and the administrator chooses among offered accounts
- **THEN** selection uses the full provider identifier and no previous or sole account is silently selected

#### Scenario: A provider write has an uncertain result
- **WHEN** project creation was attempted with a recorded billing decision and the reply is lost
- **THEN** retry retains that decision and reconciles the original project before changing it or creating another project

### Requirement: Administrator connection prepares the installation app
The system SHALL continue a missing-app administrator connection in the same
isolated Kiri browser through official Google Console setup, register the web
client for the installation callback and automatically prepare member GWS.

#### Scenario: First administrator connects
- **WHEN** the administrator selects a vault Google account and no app is registered
- **THEN** setup continues after sign-in and successful registration leads to that member's service consent

#### Scenario: Setup is interrupted
- **WHEN** setup loses a response or is cancelled after creating a provider resource
- **THEN** a retry reconciles the confirmed resource before another creation and never claims service access

#### Scenario: Administrator authority expires
- **WHEN** the initiating administrator loses session or role authority
- **THEN** setup stops before further external changes or installation registration

#### Scenario: A client exists but local registration is temporarily unavailable
- **WHEN** Google has returned the newly created client's secret and a local save fails temporarily
- **THEN** the same attempt retains the secret for bounded local retries with fresh authority checks and never resends client creation

#### Scenario: The local registration acknowledgement is lost
- **WHEN** the private file and registered checkpoint committed but the response was lost
- **THEN** recovery verifies the same client and secret under the current lease without replacing a file or creating another client

#### Scenario: A provider list is incomplete
- **WHEN** the Console client list is loading, filtered, paginated or contains skeleton rows
- **THEN** it does not prove absence and setup does not dispatch creation

#### Scenario: A gateway failure starts cleanup
- **WHEN** the gateway loses authorization or its transport while the worker may still exist
- **THEN** it keeps the active guest fence until actual teardown confirmation or the bounded cleanup deadline

### Requirement: Lost first-client secrets recover within the same identity
The installation SHALL reserve bounded recovery under fresh administrator and
lease authority only when its private registration is absent. Recovery SHALL
retain the same setup, client, callback and account and never replace a saved
registration, delete an existing secret, or expose the new secret to a model.

#### Scenario: One existing masked secret and no private registration
- **WHEN** two stable reads confirm exactly one enabled old secret on the exact client
- **THEN** the worker adds at most one secret, privately captures its own NEW card and verifies the unchanged old row plus the captured suffix after reload before saving

#### Scenario: Response lost or two secrets already exist
- **WHEN** capture is uncertain, the worker has already spent its write, two secrets are present, or three durable recovery attempts are exhausted
- **THEN** setup pauses truthfully without another Add, another client, deletion or disabling

#### Scenario: Another administrator saves while recovery is pending
- **WHEN** a valid private registration lands after recovery reservation but before its save
- **THEN** a differing recovered value is refused and the saved registration remains byte-identical

### Requirement: Reuse the application without sharing member grants
The system SHALL share the installation application through the existing GWS
distribution while keeping each member's vault and service tokens private.

#### Scenario: Google requires public branding information
- **WHEN** External publishing requires a homepage, privacy policy and terms
- **THEN** the installation supplies its own anonymous `/about`, `/privacy` and `/terms` pages with accurate data-use and separate-write-consent disclosures, without loading member data or claiming Google verification

#### Scenario: An anonymous visitor opens public information
- **WHEN** a visitor follows the public homepage and policy links without a session
- **THEN** the information remains readable across mobile layout and reload, no member or model request is made, and existing host, origin, method and API-route restrictions remain enforced

#### Scenario: Publication was interrupted before confirmation
- **WHEN** an unconfirmed publication setting has a fresh exact readback showing it is absent
- **THEN** the same saved app can retry that idempotent setting under fresh authority with at most three durable attempts in total; a positive readback settles without another write, and project/client creation is never replayed by this recovery

#### Scenario: A managed app is not published
- **WHEN** a managed checkpoint exists but Domains and Publish are not both confirmed, or the registry cannot be read
- **THEN** every member consent-start path refuses to issue a new authorization URL, even if a private client file already survived an interrupted save

#### Scenario: Another member connects
- **WHEN** an app is registered and another member connects Google
- **THEN** the member uses that app and independently consents with their own Google account

#### Scenario: Provider restricts the audience
- **WHEN** Google requires testing membership or verification
- **THEN** the connection identifies the unfinished provider step without substituting another member's grant

#### Scenario: A limited shared installation is ready for regular use
- **WHEN** the installation administrator completes setup for invited users with independent Google accounts
- **THEN** the shared External app has the intended production publishing status and exposes applicable provider warnings and limits without requiring each member to be manually registered as a test user

### Requirement: Installation vault access remains member scoped
The installation SHALL support vault enrollment for authenticated members without
restarting or assigning a new adapter per member. Server-owned token mappings
SHALL bind account, tenant, volume, generation and registry authorization epoch.
The authority protocol SHALL NOT accept a member selector from the caller.

#### Scenario: A member joins after gateway startup
- **WHEN** an active member with the installation vault origin signs in
- **THEN** their handoff and vault session resolve only to their own private runtime

#### Scenario: A handoff or grant collides
- **WHEN** another member returns a token already registered for another scope or kind
- **THEN** registration fails and the existing mapping is preserved

#### Scenario: An event acknowledgement fails
- **WHEN** a member runtime fails to acknowledge a delivered vault lifecycle event
- **THEN** another member can receive their event and the failed durable event remains retryable

#### Scenario: Vault enrollment does not enroll a browser
- **WHEN** installation-wide vault access is enabled but only an owner-specific Google helper exists
- **THEN** the helper does not receive another member's request

### Requirement: The public browser launcher pins one local engine
The installation SHALL select its supported local engine in trusted deployment
configuration. Launch, inspection, profile lifecycle and cleanup SHALL use the
same engine. Engine support SHALL require actual Linux containment and Rust
launcher acceptance, preserving the browser sandbox and private-network denial.

#### Scenario: A legacy Docker configuration is loaded
- **WHEN** the deployment JSON omits engine selection
- **THEN** the Docker endpoint and prior deployment fingerprint remain unchanged

#### Scenario: A Kiri browser deployment starts
- **WHEN** the selected engine is Kiri
- **THEN** the launcher requires the supported actual Kiri component version and uses its fixed local socket for all resource operations

#### Scenario: A member tries to supply an engine endpoint
- **WHEN** an arbitrary socket path or remote URL is supplied as engine selection
- **THEN** configuration is rejected before launching a worker

#### Scenario: One of two members closes their worker
- **WHEN** the first member's worker shuts down in an accepted Linux deployment
- **THEN** the second member's page stays available and the cleaned slot can be reused

### Requirement: Member allocation survives a compatible Kiri release
The member provisioner SHALL accept only explicitly verified Linux Kiri API
versions. A compatible release SHALL preserve the existing allocation identity,
UID, volume and inspected isolation contract. Browser and member engine support
SHALL be stated separately.

#### Scenario: A compatible member engine is used
- **WHEN** the member provisioner runs on verified Kiri 0.2.1 or 0.2.2 with Linux API 1.47
- **THEN** it reconciles the original allocation profile without replacing identities or weakening inspected limits

#### Scenario: An unverified engine is presented
- **WHEN** the Kiri version, operating system or API version is outside the verified set
- **THEN** member provisioning stops before creating or starting a container

#### Scenario: One member requires recovery
- **WHEN** the lifecycle service recovers a stopped member in an accepted deployment
- **THEN** the member keeps its allocation and session data while the running sibling retains its process and access


### Requirement: A fresh capsule owns each authentication secret
The installation SHALL use a fresh official Chrome capsule and a one-use guest
recipient for every Google connection. Ciphertext SHALL authenticate the
installation, account, tenant, volume, generation, authorization epoch,
challenge, member and selected Google email, and callback origin. The private
key and decrypted password SHALL stay inside that capsule.

#### Scenario: A vault prepares delivery
- **WHEN** the gateway reserves a recipient
- **THEN** Chrome remains stopped until successful decryption and a separate reauthorized release

#### Scenario: A different allocation receives the packet
- **WHEN** another allocation, challenge or recipient key attempts decryption
- **THEN** the packet is rejected before Chrome starts

#### Scenario: A Google password page identifies another account
- **WHEN** the visible password recipient differs from the selected vault account or Google rejects the browser
- **THEN** no password is submitted and the attempt ends without fallback

#### Scenario: A connection is cancelled
- **WHEN** the controller closes or a termination signal arrives
- **THEN** the owned browser and temporary profile are removed, another member remains unaffected, and a success receipt is not emitted

#### Scenario: The provider returns to the callback
- **WHEN** the page contains a denial or a mismatched callback state
- **THEN** the worker does not report return success; only the gateway's actual callback processing can grant a connection


### Requirement: Invited members complete admission on the web
A later member SHALL choose their account password through a private expiring
invitation link, using the existing pinned host onboarding claim. Root CLI and
developer credentials SHALL NOT be prerequisites for ordinary joining.

#### Scenario: A member opens a current invitation
- **WHEN** a current administrator creates a link for a reserved web invitation
- **THEN** the invitee can choose a password and sign in without another password change, while their Vault and data remain separate

#### Scenario: An invitation expires or loses authority
- **WHEN** its token is rotated/expired, issuer role changes, or invitee is suspended/replaced
- **THEN** initialization and activation are refused; a fresh authorized reissue can resume the same eligible reservation

#### Scenario: A password save reply is lost
- **WHEN** the member checks status or retries after initialization or restart
- **THEN** the existing password cannot be overwritten and ready status does not claim a replacement was applied

#### Scenario: The preparation timer resumes
- **WHEN** a link-issued invitation is incomplete or expired
- **THEN** the timer may prepare resources but cannot initialize or activate through its legacy operator authority

#### Scenario: A link page reloads or a foreign origin submits
- **WHEN** joining is reentered or attempted cross-origin
- **THEN** neither password nor capability is recovered from browser storage, and only exact-origin bounded private requests may admit a valid link
### Requirement: Public Linux source setup collects missing public inputs
The README SHALL lead Linux member-server installation to the existing public
source path. The Rust setup command SHALL collect missing assistant/Vault HTTPS
origins and certificate contact/terms from a terminal before product writes.
The separate local standard preview SHALL remain explicitly labelled.

#### Scenario: Operator declines certificate terms after source compilation
- **WHEN** the operator leaves the certificate terms answer empty or refuses
- **THEN** setup stops before creating product configuration or a member
- **AND** prerequisite/build outputs remain available for a later explicit retry

#### Scenario: Standard input is reserved for the account password
- **WHEN** a required public input is missing under `--password-stdin` or without a terminal
- **THEN** setup fails with an input requirement without consuming the password as public input
- **AND** complete scripted options continue to use the existing installation/replay path


### Requirement: Owner-bound Claude login uses a private cancellable child
The system SHALL reuse the protected worker configuration, owner tuple/revision,
profile, expected account and all configured model/effort choices for an official
Claude login child. The child SHALL use bounded private pipes, never infer or
publish an installation model binding, and SHALL reject an engine without the
rootless systemd/cgroup2 memory/CPU/PIDs resource contract.

#### Scenario: An owner completes the current official login
- **WHEN** one code arrives after the validated official authorization URL
- **THEN** only that original process receives it, account/catalog checks gate readiness, and no inference or binding write occurs

#### Scenario: The owner cancels or the parent connection disappears
- **WHEN** cancellation, EOF, partial EOF or a deadline occurs
- **THEN** the pending attempt is cancelled and its current login cleanup completes before reporting failure
- **AND** cancellable preflight cannot wait through another environment lock timeout after parent loss

#### Scenario: A code is invalid or repeated
- **WHEN** malformed, oversized, duplicate or out-of-order input arrives
- **THEN** the child reports a fixed failure without replaying a login or exposing raw authentication output

#### Scenario: Configuration or advertised models no longer match
- **WHEN** the profile, owner/revision, private-file policy, expected account or any configured model/effort is invalid
- **THEN** no ready binding is published and early setup failure without a result is treated as failure by the parent

#### Scenario: The parent leaves its input open after completion
- **WHEN** the child has emitted its terminal result
- **THEN** it exits without waiting for another blocking stdin read, and regular files or terminals cannot serve as its protocol channels

### Requirement: Claude preparation owns bounded resources before authentication
The installation SHALL derive a dedicated non-root service and all artifact and
storage destinations from a private root-owned request with a complete owner
pin. It SHALL verify immutable artifacts and actual rootless resource support,
retain exact recovery identity, and refuse unsupported prerequisites before
preparation effects. Preparation or a native isolation receipt SHALL NOT by
itself authenticate an account or authorize a model binding.

#### Scenario: Fresh preparation and interrupted replay
- **WHEN** an authorized root parent prepares a new owner and repeats the same request after interruption
- **THEN** the existing exact service identity and subordinate ranges are retained, unknown users or changed owner fields are refused, and the response remains preparation-only

#### Scenario: Older native user allocator uses its documented defaults
- **WHEN** a supported older useradd lacks the system-subid option and its trusted allocator files omit the SUB_UID/GID settings
- **THEN** preparation recognizes the native defaults, leaves allocation and locking to useradd, and verifies actual ranges before worker use
- **AND** existing empty subordinate-ID databases are accepted while missing or untrusted files and malformed, ambiguous or inadequate explicit settings refuse before allocation

#### Scenario: Authentication files exhaust capacity
- **WHEN** the dedicated authentication filesystem reaches its byte or inode limit
- **THEN** writes fail within the fixed filesystem, other member data is untouched, and removing only the owner's failed data permits a verified retry

#### Scenario: Worker cannot use its prospective runtime
- **WHEN** artifacts are inaccessible to the actual service UID, a service file changes, or native confinement cannot be proven
- **THEN** no ready model binding is written and the precise preparation stage remains incomplete

#### Scenario: Probe interruption and existing login data
- **WHEN** a credential-free probe is interrupted or admission is requested on a root containing a login environment
- **THEN** the former reuses one reserved probe identity and requires owned-container cleanup before directory removal, while the latter is refused without sweeping credentials


### Requirement: Installed Claude setup is session-bound and recoverable
The existing administrator gateway and UI SHALL bind Claude login to the
initiating session and full owner. Ready SHALL require durable exact model
configuration and the root worker mark. Polling SHALL NOT finalize setup.
Every connection mode SHALL reserve unfinished or unreadable operations for
authenticated recovery.

#### Scenario: An administrator reopens a running login
- **WHEN** the initiating page reloads or a second session reads the operation
- **THEN** the original session resumes its URL while the second sees progress without code or live-cancellation authority

#### Scenario: Preparation is cancelled or its initiating session is revoked
- **WHEN** cancel or authority loss occurs before publication
- **THEN** owned host work is awaited and cleaned before a successor can start, and no ready model configuration is written

#### Scenario: A gateway restarts during login or publication
- **WHEN** a current session of the exact full owner recovers the stored operation
- **THEN** an unpublished attempt is retired, an exact published config is durably rewritten and root-marked, and unreadable or mismatched config stays pending without destructive cleanup

#### Scenario: A terminal receipt or publication sync fails
- **WHEN** terminal persistence cannot complete or a config rename is visible after a sync error
- **THEN** setup stays reserved until durable reconciliation, including against concurrent API-key setup, and status polling cannot manufacture a ready state

#### Scenario: A new request never reached the host
- **WHEN** cleanup encounters only an earlier request's root record
- **THEN** it leaves that record untouched and releases only the unseen request, so a failed start cannot permanently block retry

### Requirement: An owner explicitly disconnects a managed Claude subscription
The administrator UI SHALL offer explicit disconnect of its owned subscription.
The system SHALL reserve the operation, verify full ownership, record exact
intent and durably unpublish before retirement. Completion SHALL require exact
root cleanup, public catalog withdrawal and a durable terminal receipt. Polling
SHALL remain read-only; old public catalog revisions SHALL NOT be republished.

#### Scenario: Disconnect loses its response and is replayed
- **WHEN** the owner resubmits the same disconnect operation and target
- **THEN** the same intent is resumed without starting another cleanup, and changed targets or foreign owners are refused
- **AND** the fresh operation ID is distinct from the old connection revision, with no new setup until cleanup and terminal persistence finish

#### Scenario: Disconnect is interrupted before root cleanup
- **WHEN** the gateway restarts after intent or durable unpublish
- **THEN** a current authorized session of the exact owner can resume the original target, and no new setup starts while cleanup remains unconfirmed

#### Scenario: The configuration cannot be trusted during recovery
- **WHEN** a disconnect intent was durably recorded and the config is unreadable or differs from the exact intended target
- **THEN** the operation remains recoverable without removing that target's credentials or claiming completion

#### Scenario: The owner reconnects after completed disconnect
- **WHEN** root cleanup and terminal persistence have completed
- **THEN** the UI returns to connection choices and a fresh subscription login reuses only the same owner's prepared runtime with a new login environment

#### Scenario: A deployed version-two connection receipt exists
- **WHEN** the updated gateway reads a valid version-two connect receipt
- **THEN** it preserves that connection's ownership and recovery semantics, without interpreting it as a disconnect or accepting legacy unbound version-one authority

### Requirement: Claude authentication stays inside the member Kiri screen
The existing subscription flow SHALL use fresh Kiri Chrome for the initiating
session and deliver the matching official callback privately to its CLI child.
Authentication URLs, codes, frames and input SHALL NOT enter conversations,
model requests, receipts or logs. Frame/input access SHALL require the current
owner and initiating session. Uncertain input SHALL NOT be replayed.

#### Scenario: Fresh setup and same-session reload
- **WHEN** the administrator starts their own subscription login and reloads the management page
- **THEN** the same session resumes its existing Kiri screen and no second login or profile is created

#### Scenario: Another session cannot view or control authentication
- **WHEN** another session of the same owner or a different member requests a frame or input
- **THEN** it receives no authentication frame, URL, code or control; the initiating session remains usable

#### Scenario: Only the matching callback completes the official child
- **WHEN** the private browser reaches the official top-level callback with its original state
- **THEN** the one-use code reaches the existing child once and never appears in the screen API
- **AND** a wrong state, duplicate parameter, alternate origin or repeated callback cannot activate a connection

#### Scenario: Cancellation and interruption retain cleanup ownership
- **WHEN** cancellation, authorization loss or gateway restart interrupts authentication
- **THEN** the browser and official child are cleaned before the setup slot is released
- **AND** unconfirmed cleanup remains recoverable rather than being reported as completed

#### Scenario: Official sign-in opens a related provider window
- **WHEN** proof-validated input in the initiating login opens a related provider window
- **THEN** the same private Kiri screen follows that window without replaying its navigation or borrowing another profile
- **AND** returning to the original window invalidates the previous frame's input authority

#### Scenario: A provider window closes or the original window completes
- **WHEN** the related window closes or the original bound document reaches its callback while in the background
- **THEN** the controller returns to the original screen or delivers its exact callback once
- **AND** unrelated windows, popup callbacks and stale frame input cannot complete or control that authentication


### Requirement: Existing subscription replacement and preflight recovery
An externally managed subscription SHALL remain published until a verified new
connection replaces its exact revision. Managed disconnect SHALL verify the
full owner, request and worker identity under a retirement lease before saving
its intent or removing configuration. A definite refusal before any durable
intent SHALL terminate without removing credentials or blocking a later setup.
A saved intent SHALL survive lost acknowledgements and delayed fallback writes.

#### Scenario: A legacy external connection has no managed host record
- **WHEN** its owner opens Claude setup
- **THEN** the screen offers verified replacement and does not force managed disconnect or delete external credentials

#### Scenario: A managed record changes before the disconnect starts
- **WHEN** the preflight definitively fails before an intent exists
- **THEN** configuration and credentials remain unchanged and the failed operation does not indefinitely reserve the setup slot

#### Scenario: Intent acknowledgement is lost
- **WHEN** the intent reached disk but the live job or finalizer has an older snapshot
- **THEN** recovery merges the durable intent and cannot end as an unstarted failure

#### Scenario: A page displays an older connection revision
- **WHEN** another administrator changes the connection before the page submits or rechecks it
- **THEN** stale mutations are refused and rechecking refreshes the parent connection view
