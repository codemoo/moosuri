# Signed bundle download — acceptance

2026-11-15. Base:2a2f403. The integrated Linux downloader and existing installer checks passed after independent source review.

Implemented `installation download` for an explicit versioned direct HTTPS mirror,
independent existing Ed25519 pin, new private destination and optional private TLS CA.
Manifest authentication precedes JSON interpretation and binary/evidence fetches. The
fixed signed evidence chain, native manifest contract, exact lengths/hashes and existing
complete local verifier gate no-replace publication. No executable probe/install occurs.
Ordinary failure cleanup has inode-bound nonrecursive ownership. Local install/update
behavior is unchanged. See [usage and limits](../signed-download.md).

Independent review verified all nine frozen file hashes and found no material blocker within the existing trusted-owner filesystem model. Staging uses path/inode checks rather than retained directory descriptors; it does not claim protection against concurrent same-UID/root path mutation.

Actual Linux x86_64 acceptance used Rust1.89, embedded React assets, ordinary loopback HTTPS and synthetic signing/TLS keys:

| Command/filter | Result |
| --- | --- |
| `cargo test --locked -p moosuri --test signed_download` | 2 passed, including actual download→verify→explicit install and no-execution witness |
| `cargo test --locked -p moosuri --bin moosuri installation::download::tests` | 2 passed: cleanup ownership and atomic no-replace publication |
| `cargo test --locked -p moosuri --test local_bundle` | 1 passed: existing bundle installation/update/remove contract |
| `cargo test --locked -p moosuri --test installation` | 1 passed: existing installation boundary |

All four phases exited0; exact archived source hashes were rechecked after execution. They used the shared offline vendor/build lane serially. No product network bypass, private account, production service or CI change was made.

The new integration fixture needs `python3` (stdlib SSL) and `openssl` on PATH only for
test setup. It generates synthetic CA/leaf certificates, serves loopback HTTPS and
supplies the CA through the ordinary production option. It covers missing/invalid
signature, provenance/inventory tamper/missing, header size overflow, binary tamper,
partial body, redirect refusal, default TLS distrust, credential/HTTP/traversal URLs,
version mismatch, existing/unrelated destinations, an executable side-effect witness
which download/verify must not trigger, five-file download→verify→explicit install and
legacy three-file download. Unit tests cover cleanup ownership and atomic publication
refusing an existing empty directory. No unsafe HTTP bypass exists in production code.

Official signed artifacts/key/mirror, physical ARM and macOS runtime acceptance, abrupt-kill staging inspection, timed-out transport execution and the broader stable P10 gate remain separate. This establishes an explicit Rust transport path for trusted signed bundles, not official release delivery or stable product readiness.
