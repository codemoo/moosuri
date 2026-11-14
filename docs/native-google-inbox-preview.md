# Trusted-native Gmail INBOX preview (experimental)

The default-off credential broker now supports a separately selected INBOX preview: up to5 messages with Subject, From, Date and a short snippet. Personal settings `/settings/credentials` offers a distinct preview request; the original chat accepts a direct current request such as “내 Gmail 받은편지함 미리보기 보여줘” or “Please show my Gmail inbox preview.” A signed receipt completes that same waiting run with fixed literal rendering and no model continuation. Mail is untrusted display data, never instructions or authorization for another action. The owner can reopen its UI history, but preview turns are excluded from subsequent model history, conversation search/read and learned-skill proof, including after restart or when the broker feature is later disabled.

This is a bounded current-INBOX list, without a newest-first, complete-mailbox, atomic-snapshot or summary claim. No full MIME, message body, attachments, images, link fetching, arbitrary IDs/search/URLs, sending, label changes or general connector authority is added. The existing Google setup/connector is separate.

## Explicit device selection

Use the existing [Google credential setup](native-credential-google.md) and [trusted native connection](native-credential-github.md). The same structured OAuth refresh credential stays encrypted in the device vault. No secret is entered in chat, settings, argv or a plain config. After unlock, explicitly select:

```text
select-google-preview ITEM_UUID GOOGLE_SUB EXACT_VERIFIED_EMAIL
```

This selects **preview only**. The old `select-google` selects **counts only**. Their local policy bytes, service, resource digest, binding and signed operation differ. Selecting one does not upgrade or replace the other. An existing counts-only connection leaves a preview request waiting for the separate device selection, with reconnection guidance in settings. Compatible preview requests reuse the selected grant automatically without per-run approval. One currently selected account per operation/device is supported, and ambiguous compatible devices do not get guessed.

Stable subject, verified email, item/vault identity, revision/key epochs and original grant expiry remain device-local. The server learns no message ID, thread ID, account address, history ID, page token or estimate through this projection. Subject/From headers can themselves contain mail addresses as explicitly requested display content. Actual token refresh scopes, UserInfo subject/verified email, and mailbox-profile email are checked exactly as in counts before each read; no scope is inferred from the local grant. Redirects/proxies/cookies remain disabled. A refresh-token change or actual credential/scope/identity rejection suspends reuse until explicit trusted-device reconnection. Malformed, duplicate, control-character or secret-echo mail content ends only that request as unknown, with no success projection; it does not suspend the valid grant. A new explicit request can use the same selection.

## Fixed provider contract

After the unchanged refresh → UserInfo → mailbox-profile checks, preview uses only:

1. `GET https://gmail.googleapis.com/gmail/v1/users/me/messages` with fixed `labelIds=INBOX`, `maxResults=5`, `includeSpamTrash=false`, and `fields=messages(id),nextPageToken`. No query, page token or caller input is accepted.
2. At most5 `GET .../messages/{listed_id}` calls, with fixed `format=metadata`, `metadataHeaders=Subject`, `metadataHeaders=From`, `metadataHeaders=Date` and `fields=id,labelIds,snippet,payload(headers)`. IDs come only from that list, are bounded safe path components, and are checked against the returned ID. No raw/full MIME field is requested or retained.

Each provider body is bounded to32768 bytes, each HTTP request has the existing15-second timeout, and the entered preview operation has a60-second overall timeout. No subsequent page is fetched. An empty list is an observed empty response, not an estimated count. A404 or a message that no longer has INBOX is reported in `unavailable`; other transport failures produce an unknown receipt without a partial success claim. `more_available` means a nonempty next-page token was returned, without revealing or using it. The calls are separate observations and mailbox state can change between them.

The typed signed projection contains only `messages`, `unavailable` and `more_available`. Each message has `subject`, `from`, `date`, `snippet`, and `truncated`. Text limits are256/256/128/512 UTF-8 bytes respectively, with truncation at a valid boundary and an explicit notice. CR/LF/tab fold to spaces; other control characters and duplicate selected headers fail closed. Missing headers are empty display fields. A missing/invalid required message/snippet structure is rejected, never filled with invented mail. Unknown provider fields are ignored. Exact selected credential JSON/client ID/client secret/refresh token/access token echoes in any selected display field reject the result before normalization or truncation. Provider errors, tokens and unselected fields never become output/journal strings. Transient HTTP/TLS/OS copies cannot be guaranteed erased; parsed sensitive buffers use the existing zeroizing types.

Settings renders these fields as React text, with no HTML or Markdown interpretation. Original chat uses its existing React text renderer with a fixed display template, treats the source as display-only data, discloses partial/truncated/additional-page results and excludes the preview from all later model history and recall. Admission reserves8KiB for the bounded rendered result. Previously retained requests with less space settle once with an honest content-free output-limit notice directing the owner to the original settings result, rather than repeatedly rolling back; a completely full output ends without adding text.

## Authority, recovery and compatibility

Preview reuses actual account/run/step/call/nonce authority, current native binding checks, dispatch commitments, entered/settled journal phases, current live-vault revalidation, explicit snapshot limits, idempotent registration CAS and exact signed receipt settlement. Lock/offline waits retain the original request; restart or a lost ACK does not create another provider entry. An entered request is never replayed; missing receipt stays unknown. Local/device revocation prevents new entry and output resurrection, while authentic late evidence can settle privately. Upstream Google revocation remains separate.

Preview uses new operation/projection discriminant4 and new registration/refresh command discriminants12/13. Existing FixedProbe/GitHub/counts request, receipt, projection and command bytes remain unchanged. The resource JSON is `{"google_inbox":"preview_v1"}`; old counts JSON remains `counts_v1`. No schema migration is needed. A separate persisted `credential_google_preview` flag defaults false for old tool profiles; the older counts flag grants no preview capability. Mission tool profiles disable both. Whole-current-message affirmative grammar is enforced by Store even when a model emits the tool for negative, quoted, translation or source-analysis text.

## References and bounded evidence

Public documentation checked on2026-11-15 without any real account or token:

- [messages.list](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/list): INBOX label filter, maxResults, IDs-only list, optional next-page token and estimated count. The reference does not promise newest-first ordering.
- [messages.get](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/get): authenticated `me`, listed message ID, metadata format and selected headers.
- [Message resource](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages#Message): immutable ID, labels, snippet and payload headers. Date is the displayed mail Date header, without interpreting it as a trusted delivery timestamp.
- [Existing refresh/identity/scope contract](native-credential-google.md#fixed-provider-contract).

Synthetic fixtures cover fixed paths and bounds, empty/moved/deleted/unknown results, scope/identity failures, secret echoes including beyond the truncation boundary, same grant reuse, old counts-grant denial, exact receipt/restart without replay, literal display, original-chat wait/restart/cancel/late duplicate settlement and the existing counts path. Independent binary-encoder hashes check old signatures and the new field order. Execution evidence is recorded in the review/change tasks after checks complete; this document does not certify real Google consent, quota, accounts, packaging or every P8 adapter.
