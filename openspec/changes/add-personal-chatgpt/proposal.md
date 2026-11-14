# Personal ChatGPT same-host text

## Why

The reviewed OAuth and plan protocols cannot yet power an owned saved conversation.
Users need an independently scoped personal connection without changing shared API flows.

## What Changes

- Add schema32 owner-scoped registration, sealed session, refresh/CAS and preference state.
- Connect native same-host loopback OAuth, personal text proof/selection and durable streamed chat.
- Fence disconnect, session/epoch changes and restore; preserve stale choices without fallback.
- Keep personal tools/media/background and remote VM transfer unavailable in P1.

## Impact

F01/F02/F03 model and isolation contracts; authentication, storage, restore, CLI and personal settings.
No CI enablement, deployment, Codex token extraction or live account acceptance.
