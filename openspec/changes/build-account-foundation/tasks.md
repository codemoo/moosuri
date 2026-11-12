## 1. Implement
- [x] 1.1 Add shared Rust DTOs, bounded SQLite writer, private files, schema and real file-backed isolation tests.
- [x] 1.2 Implement bootstrap, Argon2 authentication, limited temporary sessions and epoch-journal crash handling.
- [x] 1.3 Add loopback CLI/API, exact-origin/cookie/rate-limit controls and failure-focused HTTP tests.
- [x] 1.4 Build React login/password-change/account/conversation flows with stale-response tests and embedded assets.

## 2. Verify and record
- [x] 2.1 Run Rust format/check/clippy/tests, generated DTO consistency, React typecheck/tests/build and source secret scan.
- [x] 2.2 Independently review authentication, storage, tenant and browser boundaries; resolve material findings.
- [x] 2.3 Run clean installation/start/login smoke checks, record platform limitations and prepare the tested unit for commit/push with CI disabled.
