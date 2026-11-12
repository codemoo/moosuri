# Shared model connections

The administrator manages installation API/local connections at `/admin/providers`.
Each member chooses a verified model and effort at `/settings/model`, or explicitly
uses the installation default. This unit provides setup and synthetic verification;
it does not yet connect a conversation runner or subscription login.

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
