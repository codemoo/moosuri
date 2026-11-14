# Public dependency inventory

Generate a deterministic CycloneDX1.6 JSON inventory from the four committed source
locks, without installing packages or accessing a registry:

```sh
cargo run --locked -p xtask -- sbom > /absolute/output/moosuri-source.cdx.json
```

It includes all Cargo.lock entries, both web/package-lock.json and
vault-web/package-lock.json, and tools/openspec/package-lock.json. This deliberately
covers other targets, development and frontend build dependencies as well as
application dependencies. It is not the minimal dependency graph of a particular
binary. Node is still only a build/development tool.

Components keep ecosystem/version package URLs, Cargo registry SHA-256, npm
integrity metadata and reported npm license labels. Repeated npm package versions
merge their source-relative lock locations; different versions remain distinct.
The metadata records each input lock's SHA-256. Output omits wall clock, host paths,
resolved download URLs, configuration and credentials, so identical locks produce
identical bytes. No source content, package cache or license file is copied.

Cargo.lock has no dependency license evidence: the inventory does not guess it.
License clearance/notices, vulnerability assessment, target-specific build
provenance and signed official release artifacts remain separate gates. Nonstandard
Rust sources and unsupported/corrupt lock formats fail with bounded diagnostics
rather than exporting private registry or Git addresses. Generate a fresh inventory
for each release instead of committing a stale generated copy.
