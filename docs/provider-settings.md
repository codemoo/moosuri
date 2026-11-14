# Shared model connections

The administrator manages installation API/local connections at `/admin/providers`.
Each member chooses a verified model and effort at `/settings/model`, or explicitly
uses the installation default. Verified choices are connected to saved conversations.
Personal ChatGPT text connections use the separate owner-scoped flow below.

1. Add an OpenAI API, Anthropic API, OpenRouter, or compatible/local connection.
2. Enter its API key. Compatible servers may omit a key; hosted providers require one.
3. For a local server, enter the registered base URL (usually ending in `/v1`) and
   exact permitted IP addresses. `localhost` means the machine running Moosuri.
4. Discover models, or enter an exact model ID if discovery is unavailable/truncated.
5. Verify the selected model/effort. The server sends only `Reply with the single word
   OK.`, up to 2,048 output tokens, with a 60-second limit. This can incur provider
   usage. No conversation or collected source is included.
6. Choose a successfully verified combination as the installation default or your
   personal selection. Unsupported effort is rejected; compatible text currently
   supports provider-default effort only.

For private source/memory retrieval, use **도구 연결 확인** beside a verified choice.
This additional two-round proof checks a fixed function call and a fresh challenge
returned through its result, including complete transport and exact model identity.
It is pinned to the provider revision, model and effort. Text proof alone never enables
tools; failed or revoked proof leaves capability off. New runs can use a verified
capability; old runs keep their original setting. See [knowledge agent](knowledge-agent.md).

Model discovery returns at most 512 entries from a single page and 1 MiB. A truncated
list is labeled; users can still enter an exact ID. Discovery is not a claim that a
model supports tools/media or the chosen effort. A probe confirms that the exact
request completes as a text stream and reports the same exact model ID; it cannot prove a third-party server honored
undisclosed internal settings. No automatic provider/model fallback is enabled. Alias-to-snapshot mappings are not
guessed: if a response reports a different or unknown model, select the exact
version ID from the provider catalog before verification.

OpenRouter requires explicit approved provider slugs, denies data collection and
fallbacks, and offers its ZDR flag. These are request policies rather than a guarantee
of another service's behavior. Its catalog is filtered by account settings; each
selected model still needs verification with the saved routing policy.

Editing a connection increments its revision and invalidates prior verification.
Preferences retain their old revision and become unavailable; re-verifying a model
or changing the installation default does not silently move a personal selection.
A stale editor cannot overwrite a newer revision. Saving a credential with `keep`
is rejected when its connection configuration changes; explicitly replace or clear
it. Disabling a profile stops in-flight local probe waiting within the validation
interval. An already accepted provider request may still finish or be charged.

Keys are never returned to the web client. Shared API keys are encrypted in SQLite
using XChaCha20-Poly1305 and an owner-only `provider-secrets.key` in the private data
root. Ciphertext is bound to profile identity, revision and configuration. A missing
or mismatched key fails closed. Replacing or clearing a key removes superseded
ciphertext transactionally; old WAL pages or snapshots may retain encrypted bytes,
so this is logical deletion rather than secure erasure. This protects a database-only copy; an OS operator
who can read both files can decrypt these installation credentials. It is **not**
the user-key [vault](vault-security.md), and does not change the vault's admin boundary.
Do not manually copy a live database or move production accounts into this preview;
the supported backup/recovery path is still pending.

The catalog uses OpenAI `/v1/models`, Anthropic `/v1/models?limit=512`, OpenRouter
`/api/v1/models/user?offset=0&limit=512`, or the registered compatible base plus
`/models`. Anthropic catalog and inference use the same explicit `2023-06-01`
version; newly published capability metadata is not assumed. No arbitrary next-page
URL, redirects, proxy environment, TLS bypass or provider error body is consumed.

Sources checked for this implementation:

- [OpenAI model list](https://developers.openai.com/api/reference/resources/models/methods/list)
- [GPT-6.1 Terra](https://developers.openai.com/api/docs/models/gpt-6.1-terra)
- [Anthropic model list](https://platform.claude.com/docs/en/api/models/list)
- [Anthropic effort](https://platform.claude.com/docs/en/build-with-claude/effort)
- [OpenRouter filtered list](https://openrouter.ai/docs/api/api-reference/models/list-models-filtered-by-user-provider-preferences-privacy-settings-and-guardrails)
- [Ollama compatibility](https://docs.ollama.com/api/openai-compatibility)
- [LM Studio models](https://lmstudio.ai/docs/developer/openai-compat/models)

The private user's prior `gpt-6.1-terra` / `high` preference remains a future import
input, not a hardcoded public-installation default. Only account-available verified
choices can become defaults. ChatGPT Sign in and official Claude CLI subscription
isolation remain the separate paths described in [providers](providers.md).


## Personal ChatGPT text (same computer)

Run the native helper on the computer running Moosuri and the system browser:

```sh
moosuri subscriptions chatgpt-login --server http://127.0.0.1:3210 --email owner@example.test
```

Use the exact configured local origin; the server must already be running in its
normal local development configuration. Enter your Moosuri password in the hidden
prompt, then complete official ChatGPT consent in the system browser. Tokens and
OAuth callback parameters are not printed. The helper does not read Codex login
files. Remote VM protected handoff is not implemented in this slice.

At `/settings/model`, open **내 모델 목록**, choose an exact slug and optional effort,
then **텍스트 연결 확인**. The fixed text probe uses only synthetic verification
content. After proof, choose the personal model in **사용할 모델**. Catalog presence
and OAuth alone never make a selectable verified choice. An identity-only login
shows `plan_disabled`; enable the plan grant in ChatGPT and reconnect explicitly.
To reconnect an existing registration, add its displayed `--binding` and
`--revision` to the login command. Same-host returning login preserves issued client
and verified subject, including when OpenAI omits callback client_id. An incomplete
first exchange retains its issued client for a fresh-code retry.

Personal choices are never installation defaults and never appear in another
member's settings, including an administrator's own account view. This slice is
text-only: tools, attachments and background automatic memory/missions are
unavailable for personal ChatGPT. A conversation with ready document/image
dependencies from earlier turns is also unavailable for this source; use a new
text-only conversation or explicitly select a supported shared model. Nothing
silently copies prior raw attachment text to the personal provider. Shared API/local tools and background grants
continue independently. Changing the selected source affects new runs; existing
runs keep their model/effort/connection revision. A stale or disconnected personal
preference stays visibly unavailable until explicitly replaced or cleared; no
shared model fallback is performed.

OAuth bundles use authenticated server-managed sealing under the existing private
root key. A missing/mismatched key fails closed. Native listener state is transient
and abandoned on restart; verified registration and renewable sessions persist.
Normal refresh serializes rotating-token use, atomically advances generation and
preserves model proof. An ambiguous dispatched refresh is not replayed after a
crash; the connection needs reauthentication. This includes a successful rotating
refresh whose subsequent JWKS fetch or replacement validation fails. Only a known
temporary rejection from the refresh-token endpoint itself preserves the old bundle. The optional provider refresh hint is
retained without guessing its wire format. Local budgets do not impose an OpenAI
hard output-token cap for SIWC.

**ChatGPT 연결 해제** fences new requests and clears local tokens even when remote
revocation fails. Settings reports unconfirmed remote revocation accurately; use
[ChatGPT Settings](https://chatgpt.com/settings) to disconnect the app if necessary.
Already accepted inference can still be charged. Ordinary Moosuri logout ends the
local session's request authority but does not remove the saved personal connection.
Sealed restore clears personal sessions/proofs/selections and old verified identity
seals, then creates a new runtime/host ID; reconnect in the new installation.

Synthetic signed OAuth, restart, real loopback plan transport and stored chat tests
exercise this integration. They do not establish real account/workspace eligibility,
live capability or remotely managed hosted-app approval. Independent security and
actual account acceptance remain separate gates.
