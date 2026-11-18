# Shared model connections

The administrator manages installation API/local connections at `/admin/providers`.
Administrators choose the installation default and can assign a verified shared model and reasoning effort to an individual member. A member inherits the installation default until assigned an override. Ordinary members go directly to their assistant and have no model settings or provider login controls. The personal login and connection instructions below apply to administrators managing their own account. [Kiri engine administration](../deployment/kiri/admin.md)

The **Claude 연결** button in My model also leads administrators directly to a
new Anthropic API connection. The form starts with Anthropic selected and makes
the shared connection and separate API billing explicit. The adjacent personal
subscription route reports its actual server setup state; see
[official Claude execution](claude-execution.md#owner-login-and-selection).

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
files. For a separate Linux server, use the [personal server guide](#personal-server-chatgpt-guide) below.

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
text-capable; separately select **도구 연결 확인** for a saved exact model/effort to enable bounded foreground tools. A two-round namespaced challenge proves tool results reach the final answer; catalog/text success alone never enables tools. Older admitted text runs remain text-only after proof. Available tools reuse owner-scoped source/memory/skill/artifact reads, exact arithmetic, Calendar reads/status, Gmail reads/local draft preparation and configured public web/browser capabilities. Mail dispatch still requires the existing full final preview and explicit approval of its frozen revision. Explicit current-user remember/correct/forget and artifact document/goal/follow-up create/edit use the same isolated typed intent and real Store receipts as shared models, without another approval dialog or capability probe. Artifact-write stages expose saved evidence tools only; existing external-history provenance rejection remains. An explicit current-user solo timed event command can now create, edit within the same day or delete from the connected owned primary Calendar through its existing typed action and receipt. This is apply-now Calendar work, not mission delegation or future execution. Attendees, recurring/all-day events, secondary-calendar writes, attachments and background automatic memory remain unavailable. Exact saved tool proof also permits the existing read-only mission transport when the owner explicitly delegates the original one-off or recurring task; proof alone grants no background authority. New personal tool conversations may create/control those tasks through the same current-command writer path; old Calendar-only runs keep their original scope. A conversation with ready document/image
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

The model settings distinguish verified tools, an unverified tool capability, and subscription connections with separately verified tool and image capabilities. Tool support is recorded for the exact connection revision, model, and reasoning strength. The current model can use its existing verification action. When the owner explicitly changes to a different model, the picker prefers a verified tool combination if available. Chat routes supported direct Gmail inbox requests independent of the model. For other clearly tool-dependent requests on a text-only selection, chat shows a model settings action before dispatch.

## 별도 음성 연결

관리자 모델 설정의 **음성을 글로**에서 현재 유효한 설치용 OpenAI API 연결을 선택한다. 고정 공개 문장 “The blue bird sings beside the quiet river.”가 들리는15초 이하 녹음을 선택하고 **음성 연결 확인하고 저장**을 누른다. 실제 API 응답의 단어가 일치해야 저장한다. 기대 문장을 provider prompt에 넣지 않는다. 현재 모델은 `gpt-4o-mini-transcribe`이며 text/vision/tools 연결 확인과 독립적이다. 설정 변경·API key 회전/disable 뒤 다시 확인해야 한다. 구독/SIWC token은 Audio API로 보내지 않는다. 이 설치용 API key는 현재 server-managed 경계이며 개인 vault CredentialBroker 지원을 주장하지 않는다. 제공업체 처리 비용은 별도로 발생할 수 있다.

음성 설정은 provider revision과 별도의 단조 증가 authority 세대를 갖는다. 끄기/성공한 재설정은 이전 음성 job을 취소하고 임시 글을 지우며, 이전 세대의 지연된 연결 확인 결과가 다시 설정을 활성화하지 못하게 한다. 같은 연결을 다시 확인하더라도 이전 job은 재개되지 않는다.

### Personal server ChatGPT guide

Open **내 모델 → ChatGPT 구독 연결** in your signed-in Moosuri account. On an HTTPS Linux installation the page supplies a one-line command for the computer with your browser. It downloads the pinned release, checks its digest and signature, and starts only the sign-in helper. Enter your existing SSH alias and Moosuri credentials, then consent on OpenAI's official page. The remote SSH account needs the matching receiver at `~/.local/bin/moosuri` or on its non-interactive PATH. On a same-computer installation use the local helper shown in settings. The settings page checks for a completed handoff, loads the account catalog, and lets you verify and save the exact model and reasoning effort. Account-level consent is performed in your own browser. Tool verification is separate.

### Personal OpenAI-compatible models

In **내 모델 → 개인 LLM → 주소로 연결**, enter a name, the server's API base (for example `https://models.example.test/v1`), and an optional Bearer key. The server reads `GET /models` and sends text via `POST /chat/completions` relative to this base. Verify a model, then select it under **모델** and save. The current compatible transport uses the provider's default reasoning setting. Keys and choices belong to the account that added them. Updating the destination requires a new key when a saved key exists. Disconnect removes the stored credential. Personal compatible models with verified tools can run chat, one-off and scheduled work. Enabling automatic memory extraction for a collection pins its owner’s current provider, model and effort. Changing the chat selection later keeps that collection on its original connection. Disconnecting the connection prevents new extraction calls and discards late results. In personal LLM settings, each verified model also has an image check. A successful check admits images for that exact provider revision, model and effort.

For a model on the same computer as Moosuri, choose **서버에서 로컬 모델 찾기**. Discovery reads the catalog at the standard Ollama and LM Studio loopback addresses. Select a candidate to save it and verify a short text response. The administrator can discover these local services. Members use a verified shared local model when the administrator assigns it. Personal public HTTPS addresses are resolved and checked by the server.

For a model on your own workstation and Moosuri on a Linux server you administer:

1. Start Ollama or LM Studio and load a text model. In **다른 컴퓨터의 로컬 모델 연결**, copy the signed helper command to that workstation and use the existing SSH host alias for your server.
2. Choose the detected model in the terminal and start the temporary connection. A free matching port on the server and an already verified SSH host key are required.
3. Open the helper's temporary `connection.json` under **워크스테이션 연결 파일** in your Moosuri account. Confirm the model and save. The helper forwards requests for the selected text model over SSH while its terminal is open. Ctrl+C closes it and removes the file. Disconnect the saved connection when you finish.

The helper command is also available in an installed CLI as `moosuri models workstation --ssh-host MY_SERVER`. Use `--list` for discovery alone. The local discovery and bridge currently support the default, unauthenticated loopback catalogs of Ollama and LM Studio. Other servers can use a directly reachable base URL and optional API key.
