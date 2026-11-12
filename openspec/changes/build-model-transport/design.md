## Decisions

Use independent provider state machines behind one text event interface. Preserve
answer/refusal distinction, optional usage counters and assistant phase/item IDs.
Reasoning is not answer text. Unsupported tool/media requests and unverified
effort levels fail explicitly. No provider/model defaults are invented.

The caller owns the transport future, cancellation token and bounded channel.
HTTP, idle and total deadlines cover stalled headers/body and backpressure. A
closed consumer cancels local transport. Partial observations remain usable on
failure; local cancellation does not establish remote cancellation or final cost.
There is no automatic retry, fallback or continuation. OpenRouter is restricted
to explicitly approved providers with fallback disabled, required parameters and
no data collection; its actual route and model capability still need live checks.

Hosted protocol constructors fix the corresponding official HTTPS destination.
Custom compatible endpoints require administrator registration and exact local
IP authorization or public HTTPS. Resolve once per invocation, validate every
address and pin the set. Block known cloud metadata, link-local, multicast and
unspecified addresses even when listed. Disable redirects, ambient proxies and
transport retries. Preserve TLS verification. Keep keys, prompt text and raw
provider error bodies out of Debug/error output.

SSE parsing is bounded and byte-fragment tolerant. Forward each complete event
before parsing later events in the same HTTP chunk so a later failure cannot
erase the visible prefix. Responses needs a typed terminal response; Anthropic
needs closed content blocks plus message_stop; Chat requires finish_reason and
[DONE]. OpenRouter's repeated finish_reason in its final usage-only frame is
valid accounting, not a second completion. Missing usage stays unknown.

## Limits

This is a library foundation tested against synthetic inputs. It does not create
provider profiles, store keys, list/probe live model capabilities, manage budgets,
perform OAuth/CLI subscription login, persist runs/messages or expose chat in the
React application. Those remain separate implementation units. ChatGPT plan
usage and Claude official CLI integration keep the distinct contracts in
`docs/providers.md`; an API adapter does not establish subscription support.
