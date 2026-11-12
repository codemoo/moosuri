# Typed model tool rounds

`moosuri-connectors::model::protocol::tools` implements the wire contract for
client function calls. Chat still runs the text-only path. The tool registry,
durable per-step receipts, knowledge retrieval and memory writes are subsequent
implementation units.

## API and authority

`ModelClient::stream_tool_round` takes a `ToolRequest`, explicit `ToolCapabilities`,
a bounded preview channel and cancellation token. Function support defaults false;
a text probe or model catalogue does not establish function-tool support.

`ToolRequest` contains the pinned text/model/effort request, function definitions,
optional private continuation and results for exactly its pending call IDs.
`ToolRound` returns the outcome, reported usage, completed calls and continuation.
`CompletedToolCall` has private construction and exposes call ID, name and JSON
object arguments. The runtime must still validate typed parameters, account
authority, original user intent, cumulative budgets and mutation receipts.

Text/phase/usage previews never expose executable calls or `Finished`. Calls become
available only after terminal/lifecycle validation, exact model identity,
reconciled identities/arguments and clean HTTP EOF. A later chunk's error or
duplicate terminal rejects the round. A held tail remains cancellable and subject
to existing idle/total deadlines. Text callers retain terminal-marker completion;
both paths share network/credential/size/cancellation controls.

## Provider continuation

| Protocol | Private history preserved |
| --- | --- |
| Responses | Ordered output, encrypted reasoning, assistant phase, function calls and distinct `call_id` result linkage |
| Anthropic | Ordered assistant content including thinking/signatures/redactions, then user tool-result blocks |
| OpenRouter | Assistant tool calls and reasoning details, then matching role-tool results |
| Compatible Chat | Standard assistant tool calls and matching role-tool results with explicit capability |

Sparse Responses snapshots cannot erase an earlier phase. Unknown names/blocks,
changed call identities, duplicate IDs, malformed arguments and incomplete tool
turns fail. Results must cover pending IDs exactly once. Anthropic error flags
are preserved; Responses/Chat receive bounded error envelopes.

Continuations have no Debug/Serde/TypeScript export. Explicit private-checkpoint
methods validate bounds and pin dialect/model/effort. Capsules contain private
reasoning/context, never browser data. Store must additionally bind them to the
tenant/run/revision and purge withdrawn evidence. Parsing grants no authority;
runtime must branch on outcome before continuing or reporting success.

## Bounds and validation

- 12 calls per round; 16 KiB arguments/result per call.
- 16 definitions; 64 KiB schemas/descriptions; closed, depth-bounded schema subset.
- 256 KiB private checkpoint; 512 KiB encoded protocol request.
- Existing SSE, response, backpressure and transport deadlines remain active.

These are per-round bounds; the runtime must separately limit cumulative steps,
tools, output, deadline and mutations. Synthetic streams and real loopback HTTP
cover fragmented/interleaved arguments, all four continuation encodings,
identity, phase/reasoning and checkpoint correlation. No live model was called.

Independent review found chunk-dependent authorization when transport returned at
a terminal before reading the next chunk. The corrected tool path waits for EOF.
Deterministic held-tail tests cover later errors, duplicate terminals, cancellation,
timeout and clean EOF, while separately retaining text behavior. Integrated results
and remaining work are in [status](status.md).

## Primary references

- [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling),
  [conversation state](https://developers.openai.com/api/docs/guides/conversation-state),
  [Codex prompting guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide).
- [Anthropic tool handling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls),
  [streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
  [thinking/tool workflows](https://platform.claude.com/docs/en/build-with-claude/thinking-tool-workflows).
- [OpenRouter tools](https://openrouter.ai/docs/guides/features/tool-calling)
  and [reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens).

Checked during 2026-11-12 research. Documentation is a wire-contract reference,
not proof of an individual connection's current capability or mission quality.
