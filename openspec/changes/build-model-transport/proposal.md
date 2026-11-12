## Why

Model protocols disagree about streaming completion, usage and errors. A shared
LLM connection must not turn a truncated reply into success, silently substitute
an effort/model, replay a charged request or send credentials to a redirect.

## What Changes

- Add a Rust connector crate with text-only request encoding and bounded streaming
  decoders for OpenAI Responses, Anthropic Messages, OpenRouter Chat Completions
  and a conservative local Chat Completions subset.
- Add fixed hosted destinations and explicitly registered compatible destinations,
  pinned validated DNS, credential-scoped headers and cancellable HTTP transport.
- Verify with synthetic protocol fixtures and real loopback HTTP. Keep live
  account/model capability, provider settings UI, OAuth and durable chat execution
  explicitly unavailable until the next integration units.

## Capabilities

### New Capabilities
- `model-transport`: bounded, failure-aware text protocol transport.

### Modified Capabilities
- None.

## Impact

New `moosuri-connectors` Rust crate and dependencies, local verification records
and developer documentation. No production credentials, legacy code, live model
calls, account schema or CI changes.
