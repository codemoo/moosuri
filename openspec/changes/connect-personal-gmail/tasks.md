## Implement
- [x] Add bounded Google OAuth/Gmail transport, MIME freezing and explicit incomplete reads.
- [x] Add personal encrypted connections, session-bound callback completion and refresh fencing.
- [x] Add immutable local drafts, human-only CAS approval and one-attempt send commitments.
- [x] Connect admitted read/draft tools through the durable agent loop without network IO in SQLite.
- [x] Add React connection/search/preview/editor and stale/purge/uncertain-state behavior.

## Verify and publish
- [x] Independently review transport, OAuth, approval, dispatch, revocation and UI boundaries.
- [x] Resolve SSE delivery races, approval during editing and stale editor plaintext with regressions.
- [x] Run integrated macOS/Linux, React, DTO, spec, source/license/secret and public gates.
- [x] Publish this coherent unit with CI disabled and document its actual limits.

Real Google OAuth and email sending are not part of the synthetic test claim.
Development authorization alone does not approve a message to any recipient.
