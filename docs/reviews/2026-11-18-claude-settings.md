# Claude connection entry repair

The My model card previously hid its add button and all owned binding controls
when the optional runtime was unconfigured. A failed initial status request also
left a loading message on screen. This made an unsupported subscription setup
look like a missing feature and prevented owner disconnect after runtime removal.

The entry now opens explicit subscription/API options regardless of runtime state.
The API path uses the existing administrator-only editor, initially set to
Anthropic; it does not add another credential store. Subscription setup retains
the official isolated Linux contract. Failed reads end loading and allow refresh.
Owned binding removal remains available without runtime setup. Successful create
and disconnect receipts remain visible if a later list refresh fails. Account
changes remount form state and fence late responses through the existing scope.

Verification: 57 focused React connection/admin/member/App checks and production
web build passed. The unchanged ChatGPT guidance test contained obsolete copy and was updated
to check its current delegated-read and mail-approval boundaries. Independent review identified the distinct POST receipt shape; the UI now constructs
the pending binding explicitly and the failed-refresh regression uses the actual
reference shape. Read-only follow-up resolved the finding. Playwright with synthetic accounts verified
the desktop and 390px mobile layouts and real navigation into the Anthropic
editor. That shortcut scrolls the editor into view and focuses the API key field so the existing audio settings
do not leave the requested form below the mobile viewport.

No real Claude account login/probe or rootless production-runtime acceptance is
claimed. Configured profile, stored binding and verified text model are distinct
states. API keys and personal CLI credentials retain their existing boundaries.


The dev.10 signed preview packages contain the final embedded web build. Native
Mac ARM64, Linux x64 and Linux ARM64 QEMU exercised synthetic account login,
unconfigured Claude status, shared-provider access, exact embedded UI assets and
graceful shutdown. Intel was cross-built. External dependencies and their notice
inventory are unchanged. Physical Intel/ARM/WSL2 acceptance remains separate.

An existing Linux installation was updated through the stopped signed-update
command, with an encrypted source export and its previous signed release retained.
The same-schema data files remained unchanged during activation. HTTPS readiness,
the pre-update login session, personal model settings, owner-header enforcement,
real administrator provider access and exact UI/worker asset bytes passed. The
existing service unit and proxy configuration remained unchanged. The temporary
backup-setup identity was removed from the server; its independent recovery-device
copy is retained. The verification session was explicitly logged out.

The deployed subscription runtime remains unconfigured and reports that state in
the guide. API entry and credential form availability do not prove a user's API
key or subscription/model eligibility. GitHub Actions remains disabled.
