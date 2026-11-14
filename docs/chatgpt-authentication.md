# ChatGPT OSS authentication protocol foundation

`moosuri-connectors::chatgpt_oauth` implements the bounded official public OSS SIWC protocol. P1 connects same-host native loopback login to owner-scoped sealed storage, personal text probe/selection and durable streamed chat. Existing shared API providers remain independently available. Synthetic protocol tests are not an actual account connection, plan eligibility or model capability proof.

The protocol follows the official [OSS overview](https://developers.openai.com/siwc/token-sharing-open-source), [sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in), [profiles and sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions) and [token reference](https://developers.openai.com/siwc/token-sharing-open-source/token-reference), checked 2026-11-13. Paid or remotely managed hosted deployments have separate registration conditions; this module establishes no approval for them.

## Implemented boundary

`ChatGptAuthClient::discover()` fetches the fixed HTTPS OpenAI discovery document. Exact issuer, authorization/token/revocation/JWKS URLs and RS256/S256/public-client support are mandatory. Runtime endpoints are immutable; no caller-controlled base URL, proxy, redirects, retry or TLS bypass exists. Each HTTP operation has a 30-second complete deadline including DNS; connections use resolved public addresses pinned into the client, 5-second connect/10-second read/25-second HTTP bounds. DNS answers are limited to 32 addresses. Discovery/token/error/revocation bodies are limited to 64 KiB, JWKS to 256 KiB. Unrecognized responses produce closed errors without provider prose, response bodies or URLs.

Start the local listener **first**, then call `begin_registration` with its actual nonzero port and a persistently stored opaque host ID. The authorization URL uses `dynamic_agent_client`, `agent_name_hint=Moosuri`, the official plan scopes/resource, independently random 256-bit state/nonce/PKCE verifier and S256. Only `http://127.0.0.1:<port>/auth/callback` is accepted; hostname aliases and normalized IP spellings are rejected. All callbacks require exact state and a five-minute monotonic attempt lifetime. Duplicate, malformed, ambiguous or unexpected parameters fail. Callback scope never establishes a grant. Both accepted and rejected callbacks consume the attempt. For a new dynamic registration, the issued client ID is mandatory and cannot be `dynamic_agent_client`. It is exposed on `PendingCode::registration()` so the caller can save it **before** calling the consuming `exchange` method.

Initial exchange uses the exact code, original PKCE verifier, issued client ID, original redirect including port and resource as form fields, without a secret/Basic header. Returning sign-in uses `begin_returning` with a previously verified identity, the same host and issued client, a fresh attempt and no registration hint. Only the callback port changes. On reauthorization the callback may omit `client_id`; the attempt retains its exact pending client. Any supplied different ID is rejected. The signed subject cannot change a previously verified identity. If the first code exchange is rejected or expires before identity verification, `begin_reauthorization` consumes the retained `IssuedRegistration` and starts a fresh attempt with that issued client and host, fresh state/nonce/PKCE, no registration hint and the new listener port. It establishes the first subject only through verified JWT claims. After an identity is known, callers use `begin_returning` to preserve the known subject pin. This is recovery of an incomplete registration, not an unchecked identity import or durable owner authorization. This follows the current [callback/recovery contract](https://developers.openai.com/siwc/token-sharing-open-source/sign-in#3-handle-the-callback-and-exchange-the-code), independently checked during protocol review. This foundation does not use optional token/login hints or experimental reconsent parameters.

JWKS selection rejects duplicate key IDs, missing/mismatched IDs, non-RSA keys, incompatible algorithm/use/key operations, and private key material. Actual RS256 signature verification precedes claim use. Issuer, audience/authorized party, expiration, issue time (at most 60 seconds future clock skew), optional not-before, initial nonce and nonempty subject are verified. Refresh ID tokens, when returned, must retain the same subject/audience and must match the original nonce if they include one. A refresh without a new ID token retains the previously verified ID token; an absent refresh scope retains the original grant under OAuth scope-omission semantics. These retained values do not assert a newly extended ID-token lifetime. Missing scope on the initial response fails. Only granted response scopes enable plan access; identity-only success returns `SignedInPlanDisabled` and cannot expose a plan access token. Expired access also requires refresh before retrieval.

`TokenBundle` contains access/rotating refresh/retained ID token, verified identity, original nonce, grant, receipt/expiry times and optional opaque `earliest_refresh_at`. The optional timing hint is preserved without assuming a wire type or refresh schedule. The private carrier and authorization/attempt values have no Debug/Serialize/Clone implementations. Explicit `with_secrets_for_sealing` is the sensitive storage boundary; its view must never reach logs, models or ordinary responses. Secret carrier strings and bounded response buffers are zeroized on drop; HTTP/JSON libraries may allocate temporary copies, so this is not a guarantee of complete process-memory erasure.

Refresh sends only grant type, issued client ID, current refresh token and resource, omitting scope. A successful response returns a complete replacement carrier and never mutates the old bundle. Known terminal 400/401 token errors require reauthorization; network/deadline/429/5xx failures preserve the old carrier. No automatic code/refresh retry is made: a response lost after token rotation has an unknown remote outcome. Revocation posts the refresh token, token-type hint and issued client ID; an empty HTTP 200 confirms the remote request. A failed remote revocation remains distinguishable from success.

## Personal P1 integration

The store owns authenticated owner/session/epoch and binding-revision checks,
registration persistence before exchange, rotating-token single flight, generation
CAS, local disconnect and restore quarantine. Server/agent code performs no network
IO while holding the SQLite writer. See [personal connection instructions](provider-settings.md#personal-chatgpt-text-same-computer).

`SessionSealer` is an explicit trusted private-runtime key capability. It seals only
verified carriers and reopens authenticated records under exact owner/runtime/
binding/revision/generation context. Neither TokenBundle nor VerifiedIdentity gains
unchecked Deserialize/from_parts. Local reopen preserves prior verified identity
when a retained ID token is historically expired; it is not fresh OpenID verification
or an external credential import API. Access-token expiry still blocks dispatch.
The source discriminator is authoritative; ProviderKind remains the shared wire
family enum and cannot select SIWC by itself.

The native helper uses ordinary hidden-password Moosuri authentication and a
server-owned exact127.0.0.1 ephemeral listener on the same computer. Callback/code/
nonce/PKCE remain private; attempts are consumed and abandoned after restart.
Verified identity remains subject-pinned across disconnect and returning sign-in.
Incomplete issued-client metadata has no subject authority. A callback may omit
client_id only for the selected returning registration.

Refresh advances idle→acquired→durably dispatched before provider IO; only the exact
owner/binding/revision/generation/nonce and current initiating authority can commit
its replacement. Expiration never reclaims a dispatched rotating-token attempt.
Ambiguous rotation loss/restart requires reauthentication, while recognized temporary
HTTP rejection from the refresh-token endpoint keeps the old bundle with cooldown.
After that endpoint returns HTTP200, any failed parsing, JWKS fetch or identity
validation is `RefreshOutcomeUnknown`, including JWKS429/503; the old rotating
token is cleared and cannot be replayed. Disconnect fences first, clears
local usable tokens and reports remote confirmation separately. Sealed-current
restore removes tokens/proofs/selections and old identity/client authority, then
creates a new host/runtime; reconnect explicitly in the restored installation.

The foreground-tools increment adds a separate exact owner/binding revision/model/effort tool proof. It preserves the existing bounded Store tool slots, plan-private checkpoints, cancellation and current connection guards, resolving personal token readiness again before each model round. Existing source/skill/artifact/Calendar reads, calculation, Gmail reads/local drafts and configured web/browser tools are available only after that proof. Mail has no model approve/send tool: the owner still reviews the full frozen final preview and explicitly approves each send. The local-write increment enables explicit current-user memory/artifact mutation through the existing isolated typed planners and Store receipts. Each planner dispatch resolves personal access through the same RunLease/refresh CAS, preflights the exact plan dialect and commits a private decision capsule before trusted intent preparation. Sealed prepared memory resumes before client construction without another provider call. Artifact-write stages omit live mail/web/browser/Calendar definitions; the existing Store external-provenance rejection is retained. Calendar mutations remain a separate increment. Personal attachments and background work remain unavailable. Attachment exclusion includes
ready raw document/image history from earlier conversation turns, including queued
ancestors. Admission rejects it without changing the personal selection or routing
to a shared provider. Durable claim/retry/output guards repeat the fence before
loading private attachment bodies; the runner checks again before plan IO. Shared
conversation attachment inheritance remains supported. Protocol
namespace support alone is not a personal tool capability proof. Protected VM
handoff and independent fresh import verification are implemented below; no plaintext
token paste/upload or remote HTTPS OAuth callback is provided. Account/workspace
eligibility, actual-account OAuth/catalog/text/tool acceptance and hosted approval
where applicable remain unexecuted. The bounded authentication/storage/restore repairs
and current-root integration passed independent review; see the [P2 evidence](reviews/2026-11-14-chatgpt-vm-handoff.md).

The committed `synthetic-test-key.pk8` is a deliberately public signing fixture,
never a real credential. Optional `test-fixtures` constructors accept only exact
127.0.0.1 HTTP origins with nondefault ports; default production builds have no
alternate auth/model destination. Synthetic tests do not establish live readiness.

## Personal self-hosted Linux transfer

Run the matching installed `moosuri subscriptions chatgpt-vm --server https://your-server.example` binary on the browser computer. It prompts for an ordinary Moosuri login and an existing SSH host alias, then starts **Continue with ChatGPT** through the official exact `127.0.0.1` callback. Use `--binding ID --revision N` to reconnect an explicitly selected registration; omit both to add a distinct account. Equal email/subject never merges issued clients.

Before launching the browser, the native helper commits a ticket-separated nonce hash to the authenticated server. It persists issued client metadata before code exchange, verifies locally, and sends only a protected envelope through SSH stdin to `moosuri subscriptions chatgpt-receive --stdin`. The receiver uses the running server's ordinary owner API, without opening SQLite or receiving its root key. HTTPS uses normal TLS validation, no proxy/redirect, and SSH retains the user's established host trust. Tokens/cookies/keys are absent from command arguments and ordinary output. The helper uses a password-authenticated session with persisted `chatgpt_helper` purpose and an immutable absolute ten-minute deadline at issuance, before a cookie/response is returned. This applies to initial login and receipt reauthentication, including crashes before preparation. It permits only typed handoff operations, safe session/status and logout; password/account/admin/vault/source/chat/external authority is denied by the Store’s default operational validator and server routes. Browser and same-host P1 sessions keep their normal purpose and renewal. A fresh password-authenticated helper login can retrieve an already-imported receipt afterward.

The target independently checks fresh official signed ID evidence against the committed nonce and returning pins. This verifier is distinct from trusted local sealed-session reopening, which may retain expired historical ID evidence. A valid signature establishes subject/client/nonce; the authorized helper/OS is trusted to carry the real token-endpoint tuple. No portable signed refresh-token binding or helper-binary attestation is claimed. Transfer preserves the VM's preexisting host; current provider guidance does not offer host-specific usage attribution or revocation for transferred sessions.

Schema34 reserves a staged revision while the active target session stays usable. P1 and P2 share monotonic revision reservation above every retained stage, inherit only sealed verified subject pins, and reuse incomplete issued-client metadata for fresh codes after cancellation/expiry. Successful verification atomically installs generation1 with an HMAC-authenticated receipt and removes the target transfer key. Exact encrypted retries return that durable receipt. Session logout/epoch changes, disconnect/reconnect, stale revision and restore fence late installs; restore deletes tickets and receipts and regenerates runtime/host. Model proof and personal selection are never created by import.

The native helper rejects grant/unsupported ACLs on pinned ancestor/state/file descriptors before private reads/writes (deny-only macOS ACLs are supported), creates directories with mode0700 from the start, and checks files for owner/mode/single link. The source's owner-private encrypted journal is stored under `~/.local/state/moosuri/chatgpt-vm`. A durable pending marker precedes any transfer, and this helper has no refresh/inference/revoke path. Rerun the same command to reconcile lost acknowledgement. The helper independently fetches and verifies the target receipt before removing the encrypted renewable envelope and its transfer key; SSH stdout alone is insufficient. `--restart` explicitly cancels an unfinished attempt and starts fresh OAuth using retained staged issued metadata. Expired ID evidence is never refreshed locally to rescue a transfer. Removal is logical and does not promise SSD/snapshot erasure. Keep helper state outside VM backups and public artifacts.

Verify the target catalog and exact text model, then separately verify tools for the exact saved model/effort if needed. Save the personal selection and send a streamed foreground conversation. Tool proof is never created by transfer. Attachment history, media and background use keep their existing exclusion fences; explicit current-user memory/artifact mutations use the existing typed local planners, while Calendar mutations remain staged separately. This flow does not authorize managed hosting or extract Codex/browser credentials.
