# Trusted-native Google INBOX counts (experimental)

A separately selected [bounded INBOX preview](native-google-inbox-preview.md) is also
available. This counts grant remains limited to numeric totals and grants no preview access.

The default-off `credential-broker-foundation` feature now has one additional typed operation: read the INBOX total/unread message and thread counts of the Google account explicitly selected on the trusted native device. Personal settings `/settings/credentials` starts a durable request; a direct current chat request such as “내 Gmail INBOX 개수 알려줘” uses `google_inbox_counts` only after a whole-message affirmative grammar match and completes the original waiting run from its signed numeric receipt. A valid local grant is reused automatically without per-run approval. Google login, message content/search, sending, label changes, Calendar and arbitrary HTTP remain outside this adapter.

## Local setup

Use the reviewed-source native companion and existing pairing, independent live-vault connection or explicitly selected encrypted archive snapshot described in [native connection](native-credential-github.md). No signed native distribution or production real-account acceptance is supplied. On the trusted device, create/select a vault API-key item with exact URL `https://oauth2.googleapis.com`, empty TOTP and expiry fields, and this structured JSON in its encrypted secret field (values below are synthetic placeholders):

```json
{"version":1,"client_id":"SYNTHETIC.apps.googleusercontent.com","client_secret":"SYNTHETIC-CLIENT-SECRET","refresh_token":"SYNTHETIC-REFRESH-TOKEN"}
```

`client_secret` may be absent for an applicable public client. Unknown/duplicate fields, other versions, empty/non-ASCII-graphic credential strings and oversized values fail closed. Secret JSON is capped at16384 UTF-8 bytes; client ID/secret4096 each and refresh token8192. Obtain OAuth credentials through the provider's normal trusted flow independently; this companion does not launch browser login, accept credentials in chat/settings, grant scopes or create a refresh token. No master escrow is added. Never put a credential/password in argv, a command, logs, prompt, or plain configuration.

After native unlock, use:

```text
select-google ITEM_UUID GOOGLE_SUB EXACT_VERIFIED_EMAIL
```

This is the explicit local account/item/operation selection. The stable Google `sub` and exact verified email must be known from a trusted prior account enrollment; do not substitute email for subject identity. They stay in the owner-private local grant, with the selected item/vault identity, revision/key epoch, exact endpoints, allowed scopes and expiry. They are absent from server bindings, job metadata, chat/model output and signed result projection. One unexpired selected Google account is supported per device; another account is not guessed or automatically substituted. Multiple compatible device bindings keep a request waiting instead of choosing one ambiguously. The grant ceiling is30days in the CLI and does not extend when reconnected. After updating a refresh credential, explicitly unlock a fresh authenticated archive of the same vault and select the same item/sub/email again. Reconnection accepts monotonically newer item/key epochs, advances the same binding's epoch/grant version and preserves its original expiry. Retained preparations/commitments/receipts reconcile first; an entered invocation is never replayed. A lost registration ACK retains the exact previous/new binding CAS on the device and retries it idempotently, including after restart.

The supported chat grammar matches the complete current message, with optional surrounding whitespace and sentence punctuation. Examples include “내 Gmail INBOX 개수 알려줘”, “나의 지메일 받은편지함 개수 알려주세요”, “내 Gmail 받은편지함 읽지 않은 메일 몇 개야?” and “Please check my Gmail inbox unread count.” English show/check/read/get requests and bounded message/thread count forms are supported. Negation, quotation, translation, source-analysis wrappers and unrecognized prose do not authorize the tool, even if a model emits it. They create no broker job and no dispatch; no extra approval popup is introduced.

## Fixed provider contract

Each authorized read serializes through the existing single native journal owner. It refreshes a short-lived token with an HTTPS form POST to `https://oauth2.googleapis.com/token`, containing only `grant_type=refresh_token`, client ID, refresh token and optional client secret. The native-only body and parsed secret buffers are zeroized on drop; HTTP/TLS/OS transient copies cannot be guaranteed erased. Access tokens are not cached or persisted and are dropped after that one operation. Cookies, inherited proxies and redirects are disabled. URLs, methods and headers are fixed by code; model/server input cannot supply them.

The actual successful refresh response must explicitly report Bearer type, a bounded positive lifetime and exactly these scopes: `openid`, `https://www.googleapis.com/auth/userinfo.email` (or its `email` alias), and `https://www.googleapis.com/auth/gmail.readonly`. Missing actual scope, additional/broader scopes, failed refresh, DPoP-bound credentials unsupported by this adapter, or a changed returned refresh token rejects the credential and suspends the local Google grant. A changed refresh token is never silently discarded as a successful read or written into plaintext state; update the encrypted item through the normal vault client and reconnect locally. This strict scope policy intentionally rejects existing broad Google connector grants instead of inheriting their authority. Expired credentials are not copied to the server as a fallback.

The same access token is used as Bearer for fixed HTTPS GETs, in order:

1. `https://openidconnect.googleapis.com/v1/userinfo`: `sub` must match the locally expected subject, `email_verified` must be true, and email must match the exact local expected email.
2. `https://gmail.googleapis.com/gmail/v1/users/me/profile`: mailbox `emailAddress` must equal the independently verified UserInfo email. Email/aliases are not normalized or substituted.
3. `https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX`: the returned ID must be exactly `INBOX`. Only `messagesTotal`, `messagesUnread`, `threadsTotal`, `threadsUnread` are projected, as nonnegative signed-32-bit-range integers with unread<=total.

All provider response bodies are bounded at32768 bytes. Typed parsing ignores unrecognized provider response fields, but no provider names, descriptions, email, token URLs, errors, headers or arbitrary strings are returned. Invalid/missing numeric fields are errors, never zeros or estimated counts. These are the INBOX label's counts, not all-mail totals or a message-list estimate. Only OAuth refresh is POST; no Gmail send/write endpoint exists.

## Existing authority, waiting and recovery

Pairing/current account authority, local grant ceiling, live-vault current header and selected ciphertext/revision checks, request expiry, signed current dispatch commitment, prepare/entered/settled journal phases and revocation are unchanged. Live mode revalidates after decrypt and before provider admission. Snapshot mode is explicit and cannot detect subsequent changes to the original live vault. Ordinary transient unavailability suspends dispatch until current checks succeed. Credential rejection suspends Google reuse until explicit local `select-google` reconnection; it does not silently broaden a grant.

The Google request operation and projection have new explicit binary discriminants. All existing GitHub/fixed-probe signed variants retain their original bytes. Existing JSON resource columns accept a strict typed alternative `{"google_inbox":"counts_v1"}`; legacy GitHub `{owner,repository}` JSON remains exact, with no DB migration. Old GitHub jobs stay in the GitHub settings list and retain their service, target and original signature bytes. Google statuses/results use a distinct Gmail label. Persisted older tool profiles lack the new Google admission flag and do not gain a new capability on retry.

The original invocation/run/step/call/nonce is retained through locked/offline waiting, restart and lost ACK. Waiting returns execution capacity and uses the original chat deadline. A signed exact receipt completes the same terminal tool slot and original run with a fixed numeric summary, without another model call. Cancellation or authority loss blocks new dispatch/output resurrection; late authentic in-flight evidence may still settle privately. An entered operation is never replayed after restart; a lost receipt remains unknown. Local revoke and provider-token revoke are distinct; this adapter does not claim upstream revocation. The malicious-server-inside-an-existing-local-grant and trusted-binary/OS limits remain those of [credential use](credential-use.md).

## Public references and evidence

Verified against public Google documentation on2026-11-15, without accessing a real account/token:

- [OAuth web-server refresh and actual response scopes](https://developers.google.com/identity/protocols/oauth2/web-server#offline): fixed token endpoint/form, optional client secret, successful response scope and expiry, ignore unknown response fields.
- [OpenID Connect identity/UserInfo](https://developers.google.com/identity/openid-connect/openid-connect#obtaininguserinfo) and [public discovery](https://accounts.google.com/.well-known/openid-configuration): fixed UserInfo endpoint, stable unique subject, email/verified claims. No unverified ID-token payload is trusted here.
- [Gmail getProfile](https://developers.google.com/gmail/api/reference/rest/v1/users/getProfile): authenticated `me`, mailbox email and accepted readonly scope.
- [Gmail users.labels.get](https://developers.google.com/gmail/api/reference/rest/v1/users.labels/get) and [Label fields](https://developers.google.com/gmail/api/reference/rest/v1/users.labels#Label): exact label endpoint and total/unread message/thread counts.

Synthetic loopback tests exercise the actual refresh/UserInfo/profile/label adapter, wrong scopes/subject/mailbox, changed refresh token, redirects, malformed counts and secret-echo response fields. Native receipt/restart checks verify no replay and no token canary in the journal/output. An independent Python binary encoder provides golden hashes for all10 existing signed command variants plus legacy request/receipt bytes. The actual model-tool/runtime/Store path covers Google wait/restart, automatic grant reuse, exact original request, terminal numeric summary without another model continuation, and cancel with late duplicate signed settlement. Settings tests retain the same request UUID on network uncertainty and keep GitHub/Google jobs distinct. HTTP/default-off, shared GitHub regression, format/type/build and scoped Clippy evidence are recorded in the change tasks after execution. Synthetic evidence does not certify real Google consent/verification policy, quota, mobile/OS behavior, production accounts, signed packaging or every P8 adapter.

R1 repair evidence adds actual runtime cases in which a model emits the Google tool for negative/translation/source-analysis messages and Store creates zero jobs/dispatches. Real encrypted archive snapshots exercise a credential revision update and root rotation, retained original receipt/unknown-entry reconciliation and expiry-preserving local version changes. Store tests repeat a committed registration CAS after a lost ACK/restart, advance uncommitted requests without changing their invocation identity, and preserve committed requests/late receipts under the original binding.
