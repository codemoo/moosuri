## Why

Users want assigned work to reuse credentials already connected from their vault, without repeated selection or secret exposure to the model. The independent vault boundary must remain honest.

## What Changes

- Add opaque scoped credential bindings and a trusted-device broker for typed authenticated operations.
- Reuse valid prior bindings automatically; ask only for genuinely missing or ambiguous connection information.
- Preserve waiting, rotation/revocation, scheduling and lost-ACK effect behavior.

## Capabilities

### New Capabilities
- `credential-use`: Scoped credential-backed execution without model-visible secrets.

### Modified Capabilities

None.

## Impact

R14/R16/R20/R28, F33/F44/F48/F57/F58/F63/F64/F69. See [credential use](../../../docs/credential-use.md) and [vault security](../../../docs/vault-security.md). This is planned; no real vault item was accessed or connected during preparation.
