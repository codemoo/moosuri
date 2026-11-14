# Opt-in Web Push

The reminder inbox remains authoritative. On `/app/schedules`, **이 기기 알림 켜기**
requests browser permission only after a click and registers this account/device.
Permission is separate from app login: logout, session expiry/revocation, suspension,
password/account epoch changes or restore require a fresh account binding. Denied,
default, unsupported and offline states do not block chat or the local inbox.

Lock-screen notices contain only “moosuri / 새 소식이 있습니다. 앱에서 확인하세요.”
The encrypted push payload and notification metadata contain opaque identifiers,
never reminder titles/messages, endpoint credentials or caller-chosen routes.
A service worker checks its local binding generation and an authenticated passive
server authorization before display; it repeats the binding check after a held
response. Offline authorization failure omits this best-effort notice. No private
API, content, endpoint or subscription key is stored in the public PWA cache.
IndexedDB retains device/account/binding/generation IDs and a non-authorizing safe-integer intent revision. Clicking focuses an
existing app through a fixed message (preserving its draft) or opens the same-origin
`/app/schedules`; stale bindings do not open another account's notification.

Only explicit enrollment can attach a browser subscription to an account. A shared
endpoint moves to a fresh generation when another owner explicitly enrolls, fencing
old queued work. Lost enrollment acknowledgements reuse the exact request UUID and
payload. An older response cannot overwrite another account or a newer local
operation. Each on/off intent atomically increments its device revision in IndexedDB.
Current lifetime/generation checks surround asynchronous browser mutations and POST.
All tabs share one origin/service-worker Web Lock covering subscription lookup,
unsubscribe, subscribe and carrier capture. A newer intent waits for any already
started browser mutation and rechecks its generation immediately after acquiring
ownership, then reads or creates the resulting live subscription. Explicit off and
account-switch/logout cleanup use the same lock after immediately fencing local
authority. Ownership never expires while a browser mutation is pending. Browsers
without native Web Locks show device enrollment as unsupported before permission
or subscription mutation; there is no expiring lease fallback. Network POST/ACK
waits remain outside this browser lock and retain durable server ordering.
Durable server receipts reject new requests at or below the accepted watermark before
replacing any binding, including a delayed enrollment from another account sharing
the device. An off receipt affects only its own account and cannot retire or order
another account's binding. Exact request replay never repeats mutation; an old
enrollment replay can report off after a newer operation. Receipt ordering survives
restart and subscription-row retirement. Rotation, permission loss and restore can require reconnecting. Turning
notifications off clears the local display binding immediately and reports whether
the server acknowledged deactivation; it preserves the authenticated account lifetime so explicit on/off/on works immediately and does not change browser permission.

## Delivery semantics and bounds

A separate supervised Rust worker polls committed inbox rows every five seconds;
it performs no idle network/model calls. Enrollment starts after the current inbox
cursor, rather than pushing historical unread reminders. Polls scan at most16
bindings ×64 inbox metadata rows, never reminder bodies. Dispatch is limited to two
global jobs and one per owner; persisted owner claim order provides fairness.
Pending delivery is bounded to512/account and4096 globally. Terminal receipt pruning
keeps about1920–1984 recent rows/account; cursor + binding/notification uniqueness
prevents ordinary restart duplication. Enrollment receipts are capped at4096/account
and active devices at8/account. At capacity, push remains best effort and inbox
entries are retained.

A claim is not send authority. After request preparation, an actor transaction
rechecks current account/session/epoch/generation, unread/dismissed state, quiet
hours, current owned source preference, permanent notification suppression latches
and a one-hour notification display window, then records dispatch. Reads,
dismissals and quiet hours suppress new display; inbox text is retained. Quiet
pushes are suppressed rather than replayed in a morning burst.

**이 예약 소식 끄기** on the existing inbox page mutes one stable owned fixed
reminder or recurring mission schedule. It does not pause work. Muted publications
retain receipts/history without entering the inbox. Mute also permanently latches
Push suppression on existing source notices, including unscanned notices, and
terminalizes queued/claimed jobs atomically. Scanning, claim/dispatch and passive
service-worker authorization check both the latch and current preference. Unmute
allows future publications only: no old Push job, retry or display authorization
is revived. Already dispatched notices may have reached the service/device and
cannot be recalled; their actual result is preserved. Other owners/sources retain
their existing delivery policy. See [source mute](schedules.md#silence-one-reservations-notices).

HTTP2xx records only push-service acceptance, never phone display/read.404/410
retires the endpoint;429/bounded5xx responses use capped backoff, at most four
attempts and the same notification replacement tag. Unknown network/timeout or
restart after dispatch is terminal `unknown` and is not automatically resent.
Uncommitted claims can requeue. Provider failures never gate reminder publication
or text chat. A request already committed externally cannot be recalled; display
still requires the current binding and unread authorization.

## Network and cryptography

Only bounded HTTPS443 endpoints on approved FCM, Mozilla, Apple and Microsoft push
service hosts/paths are admitted. Credentials/fragments, generic URLs and private
addresses are rejected. Every DNS result is inspected and pinned; proxy, redirect
and implicit HTTP retry are disabled. DNS/connect/overall timeouts and unpooled
responses are bounded. Provider bodies are neither collected nor logged. There is
no production local-address bypass; loopback transport exists only inside tests.

`web-push-native0.5.0` supplies maintained RFC8291/8030 encryption and HTTP request
construction. `jwt-compact0.8.0` + RustCrypto P256 supply fixed ES256 VAPID JWT signing.
Default/RSA/CBOR features are disabled. Stable jwt-compact0.8 requires its `k256`
feature as a documented shared-helper cfg workaround; only ES256 is used by this
protocol. The earlier convenience VAPID feature pulled an unused RSA advisory path
and was removed. Exact resolved licenses/MSRV/features/advisory lookups are release
evidence, not a general cryptographic audit or device interoperability claim.

## Export and restore integration

`web-push-vapid.key` is a lazily generated runtime-only32-byte signing key (owner,
0600, regular/no-follow, fsync). It never reaches browser/source/logs or exports.
Opening Store or a stopped exporter creates no key. A marked missing/mismatched key
makes push unavailable while chat/inbox remain usable. Ordinary key-write failures
remove their private temporary file. A crash can leave `.web-push-vapid-<uuid>.tmp`;
strict root inventory rejects that unexpected file for explicit operator recovery,
and it must never be silently archived.

Runtime and stopped inventories allow this optional runtime-only member without
archiving it. Malformed, publicly readable, linked and unexpected temporary files
are rejected. The opened descriptor is revalidated before reading key bytes. `push::validate_optional_file(root)` validates an existing file only.
`push::strip_for_export(conn)` runs on the staged DB and removes subscription/session
carriers, retires the public VAPID marker and quarantines unfinished jobs.
`push::quarantine_for_restore(conn)` runs in activation after forward migration.
Fresh authenticated enrollment after restore generates a new key and subscription;
no old private signing authority or provider work resumes automatically. The integrated migration is schema23 after conversational reminders22; older
sealed-current snapshots are migrated before activation. Actual reopen, rollback,
staged export and restore checks cover these hooks.

## API and verification boundary

Current-account routes are GET/POST `/api/v1/notifications/push` (GET has device_id),
ordered POST `/notifications/push/disable`, compatibility owner-only DELETE
`/notifications/push/{binding_id}` and passive POST
`/notifications/push/authorize`. Metadata DTOs omit endpoint/p256dh/auth. Background
authorization does not extend sliding session expiry. No admin content API or
model-chosen notification/endpoint is added.

Synthetic tests cover actual signed/encrypted loopback requests and no-redirect
canaries, durable queue/receipts, account/epoch/session/quiet/read fences, lost ACK,
shared endpoints, staged authority retirement, explicit permission gesture, IndexedDB
operation races and service-worker generic display/navigation/cache behavior.
Actual iOS/Android/browser permission, background delivery, installation and display
policy remain root-owned manual/device gates. Synthetic service acceptance is not
completion of the entire notifications or PWA/device feature set.
