# Native vault core — isolated implementation evidence

2026-11-13. A standalone client-only Rust library implements explicit v2 envelopes/item encryption, header authentication, committed-header unlock admission, staged password/recovery/root changes, RFC6238 TOTP and bounded local Apple CSV preview/encryption. There is no assistant schema migration or service/model/broker route.

The [guide](../vault-core.md), [design](../vault-core-design.md) and [dependency review](../vault-core-dependencies.md) state precise formats, limits and remaining gates. Same required AES256GCM/Argon2id13/HKDF primitives do not imply old v1 wire compatibility. Current-header/candidate receipt types are trusted adapter boundaries, not an implemented authenticated storage protocol, durable journal or rollback/latestness proof.

Thirteen native outcome/attack tests pass, including NIST AES256GCM, RFC9106 Argon2id, RFC5869 HKDF and all eighteen RFC6238 algorithm/time vectors. Tests exercise ciphertext canaries/tamper, tenant/owner/item/revision/purpose binding, header epoch authentication, unsupported KDF/length rejection, exact UTF-8 password bytes, recovery verification, unchanged item ciphertext under password rewrap, pinned-inventory root rotation, strict Unicode/multiline CSV, generic exact filtering, duplicate/unsupported/blank counts, streaming byte/row/column limits, native OS entropy, entropy failure/collision, cancellation, stale import epoch and stable encrypted retry. Four compile-fail checks forbid secret Debug/Clone/Serialize and raw-Header unlock. No historical tests were copied.

Native Rust1.89 core tests and all-target warnings-denied clippy, no-default-features check, workspace fmt check, plan links and strict OpenSpec pass. Exact source/log hashes and environment are frozen in the sibling private delivery packet. No broad legacy suite or unverified WASM build is claimed.

Public registry metadata and an OSV107-node package/version check were required for dependencies; zero matching advisories were returned on the review date. AES-GCM is pinned0.11.1 with zeroization propagation; all twelve new registry nodes declare MSRV<=1.89. Argon2 owns and zeroizes its bounded block arena rather than relying on the convenience allocator. Dependency/OS/compiler/browser copies are not claimed completely erased.

Independent core review passed with no material finding. Four new adversarial API
tests passed in a separate review workspace. An independent Python implementation
using argon2-cffi/cryptography unwrapped both Rust-generated roots, verified the
HKDF/HMAC header, decrypted item AAD/payload and matched five custom
SHA512/7-digit/45-second TOTP windows. These are additional cross-implementation
checks, not a claim of full formal verification.

Root integration preserved all existing locked package versions/checksums and
added only the twelve reviewed registry packages. macOS and Linux x86_64 both
passed13 native tests and4 compile-fail API checks. Linux also passed the
no-default-features build; that is not a WASM result. Integrated clippy, fmt,
plan links and strict OpenSpec passed. Source/vendor hashes were verified before
and after the isolated Linux run.

Remaining acceptance is separate: synthetic old-client interoperability and explicit migration; real trusted client/Worker/React lifetimes and entropy/performance; standalone vault auth/DB/journal/nonce/CAS/archive/backup/restore; independently trusted distribution; and scoped credential broker/typed adapters with real commitment/revocation/lost-ACK handling. The full vault and F44–F51/F63/F69 are not marked complete. No live secrets/CSV/accounts/provider, child agent, external send, commits, CI or publication occurred.
