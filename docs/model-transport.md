# Model transport foundation

This Rust library supplies provider verification and durable text chat. Its
[typed tool-round API](model-tools.md) also supports the bounded agent runtime.
Tests use synthetic loopback services without live
API keys or subscriptions.

## Contract

`moosuri-connectors::model` separates request/stream protocol handling from HTTP.
The caller supplies an exact model ID and explicitly verified effort choices.
Omitting effort means the provider default; an unverified selection fails before
network dispatch. There is no hardcoded public default model. Text-only input
excludes tools/media; tool-looking prose never executes a tool.

| Protocol | Request/stream behavior |
| --- | --- |
| OpenAI Responses | `stream:true`, `store:false`, input array, reasoning effort; item IDs and assistant phase remain available; completed/incomplete/failed are distinct |
| ChatGPT plan protocol | Explicit fixed-origin constructor; separate catalog, developer input, omitted unsupported output cap and fixed namespace tools; OAuth/selection still pending |
| Anthropic Messages | separate system text, `max_tokens`, model-verified `output_config.effort`; indexed block lifetimes and cumulative usage; `message_stop` required |
| OpenRouter Chat | model-verified `reasoning.effort`, approved provider list, fallback disabled and required parameters; repeated finish reason in final accounting is accepted; `[DONE]` required |
| Compatible/local Chat | conservative Chat Completions text SSE, one choice, explicitly registered endpoint; unverified effort unsupported; unknown usage remains unknown |

There is no native Ollama NDJSON decoder or claim that every compatible server
implements all parameters. Connection probes must verify the selected model/server.
ChatGPT plan authentication and Claude official CLI execution remain separate pending
integrations in [providers](providers.md). The plan protocol alone enables neither.

Events distinguish text, explicit refusal, message metadata, optional usage and
terminal outcome. Reasoning is not exposed as answer text. Output budgets, frame
sizes/counts and malformed lifecycle behavior are bounded. Interrupted streams
are errors, even after useful partial output. An HTTP chunk containing a good
frame followed by an error still delivers the earlier frame. The decoder returns
valid prefix events alongside a later error, independently of chunk boundaries.
Responses done/final snapshots must agree with the bounded content accumulated
from deltas; contradictory final text is a protocol failure.

## Network and cancellation

Hosted clients use fixed official HTTPS origins. Custom clients require a newly
supplied endpoint-specific credential and either public HTTPS or an exact list
of intended local/LAN/Tailscale IPs. URL credentials, query/fragment, unsafe
schemes, link-local/unspecified/multicast destinations and known cloud metadata
addresses are rejected. IPv4-mapped IPv6 is normalized before policy checks.

Each invocation resolves the host once, validates every answer and pins that
address set in its HTTP client. Redirects, ambient proxies and transport retries
are disabled. TLS verification remains enabled. OpenRouter requires an explicit
provider/endpoint list, `allow_fallbacks:false`, `require_parameters:true` and
`data_collection:deny`; zero-data-retention may also be required. These parameters
request a route policy; actual provider eligibility/routing still needs live
validation and later receipt/accounting integration.

The caller owns the future, cancellation token and bounded event channel. Closing
the consumer stops waiting for DNS/request/body work; explicit cancellation does
too. An OS resolver already running may finish in the background, but the
cancelled invocation does not proceed to HTTP dispatch.
Default connect/idle/total deadlines are 10/30/180 seconds, with an 8 MiB wire
ceiling. The text encoder caps requests at 512 KiB; transport also rejects final
bodies over 1 MiB. SSE lines/events are capped at 64/256 KiB and accumulated
answer/refusal text at 1 MiB. The total deadline also bounds a
blocked consumer. No background retry, model fallback or continuation is started.
Tool rounds additionally require clean HTTP EOF before returning completed calls;
a terminal marker followed by a later error cannot authorize execution.
Dropping local transport does not prove remote processing stopped or final usage
is zero; delivered output/usage remain partial observations.

Keys have no Debug/Serialize representation and enter only the configured auth
header. Error values omit URLs, prompt text and raw provider bodies. Dependencies
can hold transient HTTP buffers; this is not a user-key vault. Reqwest/rustls use
AWS-LC native crypto in this build, consistent with the explicit dependency
boundary in [architecture](architecture.md).

## Verification

The [ChatGPT protocol acceptance](reviews/2026-11-14-chatgpt-plan-protocol.md) records
its independent identity repair, native/real-loopback tests and integrated Linux gate.
No OAuth grant or actual subscription/model access was used for those checks.

Run `cargo test --locked -p moosuri-connectors` for synthetic state-machine cases
and real loopback HTTP boundaries. The fixtures exercise UTF-8 fragmentation,
terminal/accounting differences, partial errors, unauthorized destinations,
redirects, response limits, cancellation and closed/stalled consumers. They do
not spend tokens or read credentials. Exact integrated checks and limitations
are recorded in [status](status.md).

## Primary references checked for this unit

- [OpenAI Responses streaming](https://developers.openai.com/api/docs/guides/streaming-responses) and [event reference](https://developers.openai.com/api/reference/resources/responses/streaming-events).
- [Sign in with ChatGPT inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference): a separate future authentication path with its own field/capability limits.
- [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming), [stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons) and [effort](https://platform.claude.com/docs/en/build-with-claude/effort).
- [OpenRouter streaming](https://openrouter.ai/docs/api_reference/streaming), [reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens) and [provider routing](https://openrouter.ai/docs/guides/routing/provider-selection).
- [Ollama compatibility](https://docs.ollama.com/api/openai-compatibility), [LM Studio Chat](https://lmstudio.ai/docs/developer/openai-compat/chat-completions) and [vLLM compatibility](https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/).

Checked 2026-11-12. Primary documentation and synthetic fixtures do not establish
an individual account's model access or a real provider's current availability.
