# Add bounded official ChatGPT OSS authentication protocol

## Why

The opt-in SIWC inference dialect has no validated authentication protocol. Passing unchecked OAuth responses into it would confuse verified identity with plan permission and would omit state, registration, signature and rotation boundaries.

## What changes

Add an unwired Rust connector for immutable official discovery, PKCE/state/nonce, consuming loopback callback attempts and issued-registration handoff, exact form exchange/refresh/revocation, JWKS RS256 identity verification and private rotating bundles. Preserve identity-only plan-disabled outcomes and sanitized failure classes. Add synthetic signed-fixture and bounded loopback HTTP proofs.

## Scope

No schema, owner binding, durable persistence, listener/helper, credential import, refresh lease/CAS, selection/UI or real account login is enabled. Those integration gates remain open and are caller duties; in-memory consumption is not durable authorization. Existing API behavior remains the default.
