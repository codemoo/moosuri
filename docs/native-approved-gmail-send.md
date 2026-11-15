# Approved Gmail sending through a trusted device

This experimental, default-off broker transport uses the existing **메일 → 로컬 초안 → 전체 내용 확인 → 허락하고 보내기** workflow. Fixed-time sending uses the same complete content/timing preview and approval. There is no second mail UI, model approval/send tool, automatic fallback to a server token, or routine second device confirmation.

## Connect and use

Prepare a separate Google OAuth refresh credential in the trusted device's encrypted vault using the existing structured [Google credential format](native-credential-google.md). Its actual refresh scopes must be exactly `openid`, email identity scope (`email` or `https://www.googleapis.com/auth/userinfo.email`), `https://www.googleapis.com/auth/gmail.readonly`, and `https://www.googleapis.com/auth/gmail.send`. Compose/modify/full-mail and missing or additional scopes are rejected. The existing three-scope read adapters do not silently gain sending authority; keep their credential/grants separate.

After trusted pairing and vault unlock, explicitly select:

```text
select-google-send ITEM_UUID GOOGLE_SUB EXACT_EMAIL
```

The companion signs a separate native mail connection registration. Its expected From address is shown in the existing mail connection selector. Actual subject, verified email, mailbox profile, scope, token type/lifetime and refresh-token identity are checked again before each operation. Registration itself is not a claim that Google has already verified those expected values. No credential is entered into the mail page, chat, argv, a plaintext config or a model/tool result.

Select **신뢰 기기 발송** in the mail page. A server Google OAuth app is unnecessary for that connection. Prepare a local draft, inspect every To/Cc/Bcc address, Subject, complete plaintext body, lack of attachments and immediate/fixed timing, then use the existing final approval button. Native connections support local drafting and approved new plaintext sending; their search/read/thread/provider-draft controls are not advertised. A model can prepare a draft for human review, but cannot approve or send.

Same-scope operations automatically reuse the selected native grant. The CLI currently selects a30-day grant. The device must be unlocked and online before the original action deadline (scheduled actions retain the existing due+5-minute limit). Final approval rejects a scheduled time outside the selected connection's grant deadline. Keep the device available; there is no failover or deadline extension. Reconnecting the same account/credential within the established grant ceiling is an internal freshness operation, not another mail approval.

## Exact action and one device

The existing frozen MIME digest binds the exact MIME bytes and thread setting; native sending rejects replies, HTML, attachments and unsupported outbound shapes. A separate fixed-order execution digest binds owner/install/account epoch, action/revision, connection/epoch, original MIME digest and Message-ID, immediate/scheduled instant/timezone, expiry and the original reconciliation fingerprint. The server delivers a bounded private MIME payload only for that owner's exact assigned, still-approved action. The device checks the execution envelope, MIME digest, supported shape and actual From identity independently.

The device-local grant caps server requests. As with the existing broker, a malicious assistant server operating inside an established grant is not claimed to have cryptographically proved a real owner's button click. A trusted native binary/OS is required; an archive is an explicit encrypted snapshot with the existing freshness limits.

The writer assigns each approved action revision to one exact device/generation/binding and invocation. Another device's connection does not inherit it. Prior to commitment, edits or timing changes create a new unapproved revision and fence the old request. Only the newly reviewed/approved revision can create its own effect. Cancel/revoke/auth changes/expiry prevent a fresh commitment. One transaction checks those pins, creates the existing immutable send evidence, records the broker commitment and changes the action to `dispatch_committed`; only then is its stored raw MIME cleared. The device has already checked the payload in memory.

After credential preflight, a signed online current check and current live-vault check run before durable device entry and the sole send POST. A paused/expired browser enrollment does not grant generic reads: native mail has a separate durable capability rooted in the existing approved action/background grant and current account/device/connection epochs. The old read broker's session checks remain unchanged. Logging out does not turn an approved fixed-time message into an unapproved message, while account/recovery/revocation boundaries still fence new work.

## Results and uncertain delivery

The companion journals only the small request/commitment and bounded signed result, not the raw outgoing MIME payload. Entered work is never replayed. Losing a commitment ACK reconciles that same invocation; known non-entry ends without another POST. Losing a provider response remains `outcome_unknown`. Losing a settlement ACK or restarting the companion uploads the original retained receipt; it cannot resend. Post-entry revoke cannot recall a mail already accepted by Gmail. Authentic late receipt evidence still settles the original effect.

Success projects only validated Gmail message/thread IDs (at most256 safe ASCII bytes each); it means provider acceptance, not recipient delivery/read. Definite provider rejection/non-entry follows the existing terminal failure semantics. Network/timeout/invalid response after entry remains unknown. No provider descriptions, error strings, credentials or raw response bodies become receipts/model output.

Unknown sends retain the existing original Message-ID/fingerprint, automatic immediate/15/120/600-second budget and authenticated, rate-limited **보낸편지함 다시 확인** action. Those reads use the same selected native credential and exact original effect, with no resend authority. A fresh online guard runs after the bounded Sent list and before fetching a candidate's full raw equality evidence. The same existing conservative fingerprint oracle and sticky/conflicting evidence merge are reused. A unique complete matching Sent observation does not replace `outcome_unknown` with a send receipt; no candidate is not failure proof. Source withdrawal, connection/device revoke, account changes, expiry or restore quarantine prevent late lookup content/result publication.

## Bounds, privacy and evidence limits

Native sends retain the existing new-plaintext limits:50 total ASCII addresses,2048-byte subject,128KiB body and512KiB frozen MIME. The signed broker command limit remains16KiB; the private payload reply alone is bounded to1MiB. Native Google refresh/identity responses keep the existing32768-byte limit and15-second timeout. Fixed send/Sent requests use the existing connector's explicit no-retry/no-proxy/no-redirect transport, bounded provider bodies and30-second request ceiling; native send/Sent IO has a60-second enclosing limit after preflight. No URLs, paths, arbitrary search, attachments, reply-all or future-generated text can be supplied to this adapter.

The whole supported outbound text/envelope is checked for selected credential/client/access/refresh-token echoes before the POST. Provider receipt IDs are checked before projection. OAuth values stay on the trusted device; transient HTTP/TLS/OS copies cannot be guaranteed erased. Outgoing mail content intentionally remains in the assistant service's existing private draft/action workflow. This is not a zero-knowledge mail composer. No real account, token, provider or recipient was used in development fixtures.

Schema56 adds explicit native transport/connection pins and immutable per-revision device effects. New signed operation/projection/command discriminants append to the existing protocol; old read/GitHub/count/body signature bytes remain unchanged. Restore quarantines native connection authority and uncommitted approvals; it never revives dispatch. Root-owned migration integration keeps schema55 before56.

Integrated Linux loopback provider + signed device-command/Store outcomes passed for exact approved bytes, immediate/fixed timing, unknown and Sent equality, dual devices, edit/revoke, commitment/settlement ACK loss, restart and late receipt, scope/mailbox failures and encrypted restore. Independent boundary review, affected all-target Clippy, assistant/companion builds, exact API-type parity and the React build/mail UI checks also passed. [Acceptance evidence](reviews/2026-11-15-native-approved-gmail-send.md) separates fixture repairs and remaining gates. Synthetic results do not certify real Google OAuth policy/quota/SMTP delivery, operating systems, signed packages or production readiness.
