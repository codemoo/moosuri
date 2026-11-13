# Vault core dependency/security review

Reviewed2026-11-13 for native Rust1.89. All direct versions are exact in the core Cargo.toml and the resolved graph is fixed by Cargo.lock. Against the worker baseline, twelve public registry packages are added; no preexisting package version/checksum is removed or upgraded. Existing lock dependency edges gain version qualifications where duplicate major versions now coexist. No Python/Node server, OpenSSL/SQL crypto FFI, network entropy or key service is introduced.

| Added crate | Pinned lock | Declared MSRV | License choice |
| --- | --- | --- | --- |
| aead | 0.6.1 | 1.85 | MIT OR Apache-2.0 |
| aes | 0.9.4 | 1.89 | MIT OR Apache-2.0 |
| aes-gcm | 0.11.1 | 1.85 | Apache-2.0 OR MIT |
| cipher | 0.5.2 | 1.85 | MIT OR Apache-2.0 |
| cmov | 0.5.4 | 1.85 | Apache-2.0 OR MIT |
| cpubits | 0.1.1 | 1.85 | MIT OR Apache-2.0 |
| ctr | 0.10.1 | 1.85 | MIT OR Apache-2.0 |
| ctutils | 0.4.8 | 1.87 | Apache-2.0 OR MIT |
| ghash | 0.6.0 | 1.85 | Apache-2.0 OR MIT |
| inout | 0.2.6 | 1.85 | MIT OR Apache-2.0 |
| polyval | 0.7.3 | 1.85 | Apache-2.0 OR MIT |
| universal-hash | 0.6.1 | 1.85 | MIT OR Apache-2.0 |

The current stable AES-GCM0.11.1 metadata was downloaded from crates.io, not inferred from a stale version. Its zeroize feature propagates to AES and GHASH; the alloc-only AEAD build disables default/getrandom features. Native entropy is the explicit optional OsEntropy adapter over the existing rand/getrandom OS path; rand default/std_rng features and Argon2 default/password-hash/alloc features are disabled for this core. Its explicit-memory API uses the core-owned arena. RustCrypto uses portable code and CPU intrinsics; its documented constant-time multiplication assumptions exclude unsuitable variable-time-multiply processors. Native macOS was exercised; supported Linux CPU and future wasm/browser paths need their own gates.

The [RustCrypto AES-GCM security notes](https://github.com/RustCrypto/AEADs/tree/master/aes-gcm#security-notes) document an NCC audit of the primitive implementation family. This does not independently audit this new wrapper/envelope protocol or all future crate revisions. [RUSTSEC-2023-0096](https://rustsec.org/advisories/RUSTSEC-2023-0096.html) affected AES-GCM0.10.0..<0.10.3; the selected0.11.1 is outside that range and failed decrypt buffers are never returned.

A live public [OSV querybatch](https://google.github.io/osv.dev/post-v1-querybatch/) checked the107resolved core dependency nodes (including target-specific nodes) by package/version, returning zero matching advisory IDs on the review date. The exact JSON and hashes are in the private delivery packet. This is a dated dependency match, not proof of no vulnerabilities or a full-workspace cargo-audit result. No private data was transmitted.

Every newly added node declares MSRV<=1.89 (aes0.9.4 requires1.89 and ctutils0.4.8 requires1.87). The107-node closure includes dependencies without declared MSRV; compatibility of the selected native feature set is evidenced by compilation on1.89, not a fabricated declaration. License metadata offers permissive choices (MIT/Apache/Unicode/BSD as applicable); no new copyleft-only package was added.

Existing primitives are Argon2id0.5.3, HKDF0.12.4/HMAC0.12.1/SHA2-0.10.9, SHA1-0.10.7 only for RFC TOTP, zeroize1.9.1, canonical Base64-0.23.1 and data-encoding2.11.1. Argon2 zeroize alone does not wipe its convenience allocator's arena, so this core supplies a Zeroizing block arena to the explicit-memory API. Secret wrappers do not derive Clone/Debug/Serialize, but primitive internal/register/OS/JS copies are not claimed fully erased.

## Separate service/WASM client dependency delta

The service/client increment adds only three local workspace packages to Cargo.lock; every registry package version/checksum is unchanged from published0d8d11c. It reuses pinned wasm-bindgen0.2.130 and the existing HTTP/SQLite/React graph. The actual Rust1.89 wasm32 build is now exercised; that does not imply mobile performance or independent signed-client delivery. `vault-web` pins the same React19.3.1/TypeScript5.9.2/Vite8.3.8/Vitest5.1.0 versions as the assistant bundle and its `npm ci --ignore-scripts` audit reports no vulnerabilities on the check date.

A separate dated OSV query of all357 registry lock nodes found two inherited maintenance advisories: basic-toml0.1.10/RUSTSEC-2026-0374 and proc-macro-error2-2.0.1/RUSTSEC-2026-0173. Both report unmaintained dependencies, rather than a new vault vulnerability. They remain explicit dependency maintenance work; this increment does not silently upgrade the established graph or claim a clean workspace advisory scan. The private freeze packet retains exact OSV request/package/result hashes. Source licenses and declared MSRV for new external packages do not change because no external package node was added.
