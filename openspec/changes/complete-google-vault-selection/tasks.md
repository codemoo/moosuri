# Work log

- [x] Implement all-account vault selection and email/password creation.
- [x] Bind the selected email through Kiri and the OAuth persistence boundary.
- [x] Keep the vault entry available without an installation OAuth app.
- [x] Run Linux vault, Kiri, OAuth and real Chromium scenarios.
- [x] Complete independent Claude review and deploy the verified unit.
- [ ] Confirm real Google sign-in, consent and one Gmail read.

The last item requires actual Google credentials in the newly reset vault and
an installation OAuth client. Neither is inferred from synthetic test results.

Linux evidence (2026-11-26): Kiri helper 19, gateway 24, OAuth persistence 1,
vault selection 9, connection UI 14, preview 65, handoff 19 and popup-origin 1
passed. Chromium exercised zero/one/multiple Google entries using distinct
synthetic emails and independently decrypted the handoff to check its binding.
Claude Sonnet 5.5 / medium independently reviewed the authorization/storage
change and handoff. The disabled-button scope, definite Status termination and
ambiguous HTTP timeout/server-error recovery findings were fixed and verified.

Schema registry (4), schema103 inventory, verify-plan and the new OpenSpec
validation passed on Linux. All changed Rust files passed formatting checks;
the workspace-wide check still reports pre-existing formatting in unrelated
browser/vault modules. No unrelated formatting was changed.

Production gateway, vault, lifecycle, member image and protocol-v3 helper were
deployed together with a stopped private-data backup. The member reached ready,
schema103 and unchanged mounts/UID were confirmed, public assets matched the
build, and an unauthenticated valid connection request returned 401. Kiri's
Gmail button opened the locked member vault even without an OAuth app; the test
was cancelled before any credential delivery. [Work log](../../../docs/reviews/2026-11-26-google-account-selection.md)
