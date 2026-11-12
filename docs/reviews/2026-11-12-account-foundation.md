# Account foundation verification — 2026-11-12

This record covers the new account foundation, not assistant quality or a
production release. All accounts, passwords, databases and HTTP fixtures were
synthetic. No production data, model credentials or legacy tests were copied.

## Automated checks

Rust 1.89.0 `cargo fmt --all --check`, workspace/all-target clippy with warnings
denied, and 17 tests passed on macOS. The same locked sources and embedded React
build passed all 17 tests in Linux x86_64 using a resource-limited container:

`rust@sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff`

The 17 tests comprise one real CLI/HTTP/restart installation journey, seven HTTP
boundary cases and nine store cases. A store test invokes a helper in a separate
process; that invocation is not counted as an additional scenario. Assertions
cover competing writers, private paths, tenant isolation, temporary-credential
exchange, expiry/revocation, journal gaps and truncation, exact origin/host,
trusted proxy/rate-limit behavior and stale-tab account assertions.

React passed 18 behavioral tests, typechecking and a production build. Rust DTO
export matches the committed TypeScript declarations. These fixtures exercise
stale responses and authentication overlap; they do not prove model behavior.

## Browser evidence

The integrated Chromium check uses the real Rust server and a dedicated local
database. It exercises invitation, mandatory password change and conversation
creation/deletion. Signing out and signing into another account in a second tab
updated the first tab and removed the previous account's conversation list.

A separate regression disabled BroadcastChannel, held the already-completed
logout response from account A, logged in as B in a second tab, then delivered
A's response. B's session remained valid, a protected conversation create returned
201, and a stale A assertion returned 401. The held response had no Set-Cookie.
Expected unauthenticated/stale-account 401s appeared in the browser console; no
JavaScript or CSP errors were observed.

Synthetic viewport checks at 1440×1000 and
390×844 cover layout/independent scrolling; they are not real mobile-device or
PWA tests. Screenshots and browser logs remain outside the public source export.

## Review and dependencies

Independent review resolved four material boundaries: rate buckets shared by
every proxy client, unrelated-directory chmod, cookie account changes across
tabs, and delayed logout responses clearing newer sessions. Authenticated
requests assert their expected account; the server compares that assertion
against the cookie-derived identity. Session reads and logout never Set-Cookie.

The initial dependency check identified
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html).
The locked build now uses rusqlite 0.41.1/libsqlite3-sys 0.39.1 and verifies SQLite
3.54+ before opening any database. A final OSV querybatch for all 153 registry
packages returned no advisory or pagination entries. This is a dated advisory
lookup, not a guarantee about undiscovered vulnerabilities. npm audit previously
reported zero findings for the pinned web dependency tree.

## Limits

No actual model, provider subscription, message run, memory, mission, vault,
scheduled job, push delivery, backup restore or production deployment is tested
by this unit. The auth journal fails closed after an uncommitted epoch; the
operator recovery command remains future work. GitHub Actions stays disabled;
the checks above were explicitly run outside CI.
