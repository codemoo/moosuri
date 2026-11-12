## Context

This implements the shared API/local provider part of R04/R05/R11/R15/R17/R18 and the P2 path in
`docs/providers.md`. Server-managed model API credentials are not user-key vault
secrets: an OS operator with the key and database can decrypt them. The separate
vault/broker contract remains unchanged.

## Decisions

- Encrypt keys with XChaCha20-Poly1305 and a separate owner-only installation key.
  Bind ciphertext to profile identity/revision/config; detect key loss and changes.
- Editing a connection increments its revision and invalidates verified choices.
  Keeping a credential across a changed destination/config is rejected.
- Default/user preferences pin a verified revision. Unavailable preferences remain
  unavailable, even if another default exists; selecting a fresh choice is explicit.
- Discover at most one bounded catalog page. Truncation is visible and exact model
  IDs can be entered. Catalog discovery never claims tools/media/effort support.
- Verification sends only a fixed synthetic prompt, requires a full completed text
  stream, and saves the exact pair only if the session and revision are still valid.
  Two probes/catalog calls may run; each is bounded to 60 seconds and 1 MiB.
- Catalog and probe use the same pinned destination and credential protections as
  inference. Refused/incomplete/truncated/error responses do not activate a choice.
- No subscription token import, provider fallback or production data migration.

## Validation

File-backed tests cover authorization, independent user selection, revision races,
key permissions/loss and authenticated ciphertext. Loopback HTTP covers catalog,
probe completion, malformed/refused output, revocation and safe errors. React tests
cover setup, model selection, stale account requests and non-echoed key fields.
Live credentials and production deployment remain a separate validation step.
