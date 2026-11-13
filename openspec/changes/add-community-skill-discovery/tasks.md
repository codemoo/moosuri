## 1. Prepare
- [x] 1.1 Document Git contribution, card UX, package boundaries and useful failure scenarios.

## 2. Implement
- [x] 2.1 Build bounded Rust package validation and exact source/version/content identity (curated four-file packages; external archives/import remain unsupported).
- [x] 2.2 Implement per-user install/update/rollback/disable/remove with SQLite and crash-safe staging (installed run pins and remove/retry revocation included; scheduling integration remains future).
- [x] 2.3 Expose readiness and install/connect/use through suggestion/capability cards at `/app/skills` (start opens a chat composer; installation itself never executes).
- [ ] 2.4 Build private-learning reconstruction/export and verify seeded private data never leaves.
- [ ] 2.5 Create trusted static release/catalog generation and license/compatibility checks.
- [ ] 2.6 Pass COMM lifecycle/isolation/UI cases before publishing support claims.

Curated-slice verification: package bounds/identity/authority rejection; two-user lifecycle, interrupted staging, update and rollback; run snapshot preservation, schema non-expansion, disable/new-admission and remove/retry fencing; HTTP authentication/current-account/no-task admission; React install/readiness/hide/late-account-response behavior. Broader COMM export/archive/release cases remain open with2.4–2.6.

Integrated curated closure: draft preservation/zero-send and direct SPA entry regressions added; independent re-review found no remaining concrete defects. macOS/Linux workspace344 passed(1 live opt-in ignored), React139 passed with type/build checks. Broader COMM export/Git/release/scheduler work remains explicitly unchecked.
