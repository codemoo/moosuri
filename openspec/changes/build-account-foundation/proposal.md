## Why

A public plan needs an executable installation and account boundary before private assistant work can be accepted. Implement the first Rust/React/SQLite slice for F01/F16/F17/F53/F56/F57/F59/F61/F64 (R01/R02/R03/R04/R05/R06/R07/R09), without pretending that model execution, vault or migration already works.

## What Changes

- A local `init` command creates exactly one administrator; skipped passwords become temporary and must be changed.
- A private, locked SQLite writer owns accounts, sessions and tenant-scoped empty conversations.
- An HTTP API enforces authenticated contexts, limited password-change sessions, exact origin and admin metadata boundaries.
- React provides login, password change, conversation management and local account invitations.
- Rust exports public DTOs to TypeScript. One Rust binary serves built React assets.

## Impact

New core/store/server/CLI crates and React build. No legacy source, live data, model credentials or deployments are imported. The existing planned scenario groups remain planned until all their wider conditions run; targeted implementation evidence is recorded separately. GitHub CI stays disabled; checks run locally per work unit.
