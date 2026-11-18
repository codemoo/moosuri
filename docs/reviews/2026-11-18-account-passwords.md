# Account password policy — 2026-11-18

Account enrollment now accepts at least eight Unicode scalar values and at most256 UTF-8 bytes. Store bootstrap/change/recovery and the bilingual installer share the Rust predicate; React matches it before submitting. Existing hashes keep their login behavior, including passwords previously accepted by the byte-based minimum. No normalization, truncation or schema migration is introduced. Separate vault password requirements are unchanged.

Validation completed before packaging:
- Native and Linux Store outcomes: exact-eight enrollment/login/change, seven-character ASCII/Korean/emoji and oversized rejection, failed changes retaining the session, successful changes revoking old sessions/passwords, and legacy multibyte hash login.
- Existing temporary-password atomic exchange/expiry regression passed; generation and first-login change requirements are unchanged.
- React authentication suite:30 passed, including blocking four emoji and accepting an eight-character replacement before workspace access. TypeScript and embedded web build passed.
- The plan verifier now includes the existing Telegram requirement/feature rows. Native Clippy passed. Independent read-only account-boundary review reported no P0/P1/P2 findings.

Package/build results are recorded separately in the preview release evidence; synthetic checks do not certify every device or live model connection.

All four native packages built successfully and passed signed-manifest, provenance, exact-archive and public-text checks. macOS ARM64, Linux x64 and Linux ARM64 QEMU packages initialized an exact-eight synthetic account and passed HTTPS-proxy-compatible web entry, login, Telegram settings, graceful stop and doctor checks. macOS Intel is cross-built; no physical Intel/ARM/WSL acceptance is claimed.

[v0.1.0-dev.9](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.9) publishes the packages and exact one-line installer pins. Registry dependency versions are unchanged; existing exact-source license evidence was reused after comparing the dependency locks. CI remains disabled.
