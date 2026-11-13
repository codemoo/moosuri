# Design

`SessionView.chat_draft_scope` is optional and is omitted for restricted sessions. Store's
`UserContext` derives it with SHA-256 over a fixed domain, opaque session digest, length-prefixed
account ID and epoch. It exposes neither bearer nor digest and is never accepted for authentication.
Repeated current-session GETs return the same namespace; another login or password rotation returns
another namespace. The server continues requiring its opaque cookie/session authority.

App confirms authentication before a recovery owner exists and waits for a successful authenticated
thread list before reading saved text into a composer. An envelope must match both exact account ID
and scope. A confirmed mismatch immediately deletes the owned storage key even when the thread list
is unavailable. Passive revalidation compares the scope in addition to account/role/restriction,
fences old request scopes synchronously, and unmounts old UI. Legacy responses without a valid scope
remain in-memory only. A failed network probe never authenticates storage or logs its contents.

An in-memory irreversible owner lease gates every storage save. Logout clears the key and retires the
lease before its network request, so transport failure, delayed POST rejection and component cleanup
cannot repopulate it. Auth hints/401/password transitions apply the same rule. Authenticated GET boot
failure retains inaccessible bytes for a later verified retry; it never loads them into the UI.
Both automatic `online` boot retries and the manual retry use this non-destructive path, including
when a boot request is still loading. They synchronously close and retire the abandoned boot scope;
only the current scope may apply a status, session, error or401 result. A delayed reply from an older
probe cannot delete recovery or replace the newly authenticated UI. The replacement boot must still
confirm the exact account/session and owned inventory before exposing saved text.

The sole versioned sessionStorage envelope retains at most16 active/new composer slots, at most128KiB
of UTF-8 JSON,8192 UTF-8 bytes per draft or pending text, and an8-hour lifetime since its last save.
The selected thread is validated against the current server list. Missing/deleted threads lose their
saved slots; a failed thread lookup also removes the affected slot. Normal typing persists
synchronously through event callbacks so page refresh does not depend on an unmount or unload hook.
Corrupt, oversized, expired, future-dated, blocked or quota-exceeded storage never blocks in-memory chat.
Over-limit slots/text are not recovered; no private text is logged. Selected/pending/current slots
are prioritized within bounds.

Pending carriers have a closed validated shape: original UUID, literal text, up to4 unique immutable
media IDs/revisions, and original artifact/schedule/mission target ID/revision/timezone context. Local
upload selection handles and media bytes are excluded. Resumed statuses are always uncertain,
including prior accepted/sending states; no restored state invokes a POST, provider or effect itself.
GET history can reconcile a matching original request, even if that run failed, without putting its
text into the composer or overwriting the next draft. The existing explicit resend uses the same
carrier and server idempotency/authorization checks; storage grants no added authority. If thread
creation had no acknowledgement, a `new` pending slot requires user intervention and may leave an
empty server thread; no automatic create/send is attempted.

sessionStorage is plaintext in a trusted tab/browser profile. Same-origin scripts and browser/OS
access can read it; this feature does not protect against XSS or a malicious client. Browsers may
clone a tab or restore sessionStorage after crashes/restarts. Recovery still requires the exact
current server session, but browser-session persistence/clearing cannot be guaranteed. Users on
shared devices should sign out; full vault encryption and closed-tab durability are separate scope.
No draft or pending envelope is sent to a service worker, Cache API, localStorage, model or diagnostic log.
