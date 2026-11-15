# Trusted-native selected Gmail body (experimental)

The owner can open the general text body of a specific message from a genuine native INBOX preview in personal credentials settings or under that preview's original chat. The owner clicks **이 메일 본문 열기**. This UI action submits only an opaque selector and the original preview/request UUIDs. There is no body tool for the model and no arbitrary Gmail message ID, search, URL, write or send action.

After the usual encrypted credential setup, unlock the companion and explicitly select:

```text
select-google-body ITEM_UUID GOOGLE_SUB EXACT_VERIFIED_EMAIL
```

This is a separate local body grant. Counts and preview selections keep their original scope. A compatible body selection reuses its grant automatically; there is no approval for each read. One currently selected account per operation/device is supported. All mail sends, through separately supported paths, still require their own final-preview approval; this adapter cannot send.

Selectors have the original preview invocation's expiry, which may be short for chat requests. Finish device setup before requesting a new preview. Settings/chat explains expired selections with **새 미리보기에서 다시 선택하세요**; requesting a new preview does not reconnect or broaden the grant. A completed preview remains readable as private display history after its selection expires.

## Selected identity and authority

New previews contain random opaque per-message UUID selectors signed inside their projections. Actual Gmail IDs remain only in the trusted device's private fsynced journal. The local mapping pins the exact preview binding, selected item, vault, Google subject/verified email, item/key epochs, device and original request expiry. It survives restart without replaying a provider operation. The body grant must use that same credential/account/vault/version and live-vault origin. A changed, revoked, suspended or expired preview grant cannot authorize a body. Counts/preview grants cannot register or dispatch body operations.

The authenticated Store only accepts a selector found in that owner's stored, authenticated successful preview receipt. It verifies original cancellation, account authority, current device/binding and expiry; dispatch rechecks those conditions under the existing writer transaction. The body request carries its exact selector in new signed operation bytes. The native adapter independently resolves that selector and checks its hidden identity before provider entry. A selector from another device is not reassigned. Unknown IDs/selector mismatches perform no provider reads. Current pairing/run authority, dispatch commitment and prepare/entered/settled recovery remain unchanged. Authentic late evidence may settle privately after revocation, but is not displayed. No schema migration is needed.

These controls cap the local grant; they do not prove owner intent against a malicious assistant server already operating inside that grant. Trusted binary/OS and explicit archive snapshot limitations remain those of [credential use](credential-use.md).

## Provider and content bounds

Every body operation repeats the existing exact refresh scope, Bearer lifetime, UserInfo subject/verified email and mailbox profile checks. Only the fixed `GET https://gmail.googleapis.com/gmail/v1/users/me/messages/{device-local-id}?format=full&fields=id%2ClabelIds%2Cpayload` follows. The returned ID must match and current labels must include INBOX. A404 or removed INBOX label returns a content-free unavailable result. Transport/parse/unsupported-content failures return unknown, without an invented summary. The operation has the existing60-second overall bound and15-second per-request timeout; redirects, proxies and cookies stay disabled.

The provider JSON response is bounded to32768 bytes. The typed MIME traversal permits at most32 parts and depth4. It supports exactly one inline `text/plain` part directly or inside `multipart/alternative` / `multipart/mixed`, UTF-8 or US-ASCII charset, and a matching strict URL-safe base64 payload no larger than16384 decoded bytes. Named files, attachment dispositions and attachment IDs are never fetched or projected. HTML-only mail, unsupported charsets and multiple plaintext bodies are unsupported. No external images, links, attachment content, HTML conversion, thread expansion, pagination or raw MIME output is provided.

The entire decoded chosen text is checked for exact selected credential JSON/client-ID/client-secret/refresh-token/access-token echoes before truncation. Invalid control characters are rejected; CR/LF/tab are permitted as literal mail formatting. At most4096 UTF-8 bytes cross the signed projection, cut only at a character boundary with an explicit truncation notice. Empty plaintext is an observed empty body. Provider error strings, headers and unselected fields are never returned. Transient HTTP/TLS/OS copies cannot be guaranteed erased.

## Private display, compatibility and evidence

Credential secrecy is distinct from mail content privacy: the selected body intentionally reaches the assistant service's SQLite receipt and owner's React UI. This is not a zero-knowledge mailbox. It is never inserted into regular chat transcript, notifications, model continuation/history, conversation recall, artifacts or learning proofs. Original-chat rendering is a separate literal React surface selected by the original preview run and selector. Neither mail body nor its links/markup can authorize actions. Settings uses the same text-only surface.

Existing preview records without selectors remain displayable and ask for a fresh preview to enable body selection. Old FixedProbe/GitHub/counts and selector-free preview signed bytes remain unchanged. New request/receipt/projection operation5 is the body; selectable preview projection uses6, while selector-free preview remains4. New register/refresh command discriminants are14/15. Their resource is `{"google_body":"OPAQUE_UUID"}`; registration uses the nil UUID as a scope marker, which cannot be used as a selected message. No dependency version or lockfile changes are required.

Synthetic fixtures cover actual fixed provider requests, plaintext/MIME/encoding/size bounds, secret echoes before truncation, deletion/moved messages, unsupported HTML/charset/attachments, separate grant denial, exact selection, automatic reuse and restart without replay. Store/API fixtures cover genuine receipt admission, arbitrary selector denial, original expiry, device revocation and duplicate late private settlement. React fixtures cover literal rendering, expired selector guidance, original-run isolation and a lost response retaining the same request identity. Existing binary golden checks protect old wire bytes. Tests use synthetic accounts, tokens and local fixtures only; real Google OAuth/quota/accounts, packaged companions and production acceptance remain unverified.
