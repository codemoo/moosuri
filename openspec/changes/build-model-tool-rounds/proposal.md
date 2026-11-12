# Change: bounded typed model tool rounds

## Why
An agent loop needs complete function-call validation and provider-specific
reasoning/history continuation before tools can access application state.

## What changes
- Add explicit function capability, definitions, completed calls and private
  continuation to the four existing dialects.
- Share HTTP/network/cancellation controls and preserve text-only APIs.
- Require bounded clean EOF before completed calls; previews never authorize work.
- Add synthetic protocol and real HTTP held-tail checks and usage guidance.

## Impact
Advances the model-facing P0/P4 substrate. It enables no user-facing tool execution,
source retrieval, automatic extraction, external action or arbitrary code. No full
feature or phase is marked complete by this library change.
