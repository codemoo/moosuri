# Change: recover current-tab chat drafts after refresh

## Why
A full page refresh previously discarded unsent composer text and the client identity of an uncertain
message submission, while saved server conversation history survived. Recovering those fields needs
an exact authenticated session boundary; an account name alone can revive another login's private text.

## What changes
- Add an optional public, non-authenticating draft scope derived from the opaque session digest,
  account identity and authorization epoch with a dedicated SHA-256 domain. Restricted sessions omit it.
- Recover only bounded current-tab sessionStorage composer/pending metadata after the server confirms
  the same session and supplies its current owned thread inventory. Do not persist response bodies,
  source artifacts, credential values, media bytes or selected new effect authority.
- Treat recovered pending submissions as uncertain. Initial recovery only reads history; resubmission
  remains an explicit user action preserving the original request UUID and immutable carrier.
- Clear recovery synchronously on sign-out, auth transitions, password mutation or401; fence late writes
  even when a logout network request fails. Same-account session replacement is also a new lifetime.

## Scope
Tab-session plaintext recovery, not a vault, offline authenticated access, automatic queue, cloud backup
or guaranteed browser crash recovery. No schema, dependencies, service-worker/private cache or
localStorage changes. Existing optional-field deployments remain in-memory only.
