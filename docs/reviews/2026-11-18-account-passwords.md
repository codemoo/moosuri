# Account password policy — 2026-11-18

Account enrollment now accepts at least eight Unicode scalar values and at most256 UTF-8 bytes. Store bootstrap/change/recovery and the bilingual installer share the Rust predicate; React matches it before submitting. Existing hashes keep their login behavior, including passwords previously accepted by the byte-based minimum. No normalization, truncation or schema migration is introduced. Separate vault password requirements are unchanged.

Validation completed before packaging:
- Native and Linux Store outcomes: exact-eight enrollment/login/change, seven-character ASCII/Korean/emoji and oversized rejection, failed changes retaining the session, successful changes revoking old sessions/passwords, and legacy multibyte hash login.
- Existing temporary-password atomic exchange/expiry regression passed; generation and first-login change requirements are unchanged.
- React authentication suite:30 passed, including blocking four emoji and accepting an eight-character replacement before workspace access. TypeScript and embedded web build passed.
- The plan verifier now includes the existing Telegram requirement/feature rows. Native Clippy passed. Independent read-only account-boundary review reported no P0/P1/P2 findings.

Package/build results are recorded separately in the preview release evidence; synthetic checks do not certify every device or live model connection.
