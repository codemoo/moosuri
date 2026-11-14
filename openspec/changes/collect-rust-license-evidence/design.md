# Design

Read the committed Cargo.lock and an explicitly selected versioned vendor tree.
Only public crates.io sources are admitted. Compare package checksum metadata,
then verify Cargo.toml and package-relative candidate notice bytes. Preserve raw
license declarations and notice text in deterministic JSON. Stable trusted source
trees remain a prerequisite; local manifests do not authenticate registry origin.

All target/dev packages are included. Missing notice/declaration evidence is
explicit; optional strict mode emits the report before failing. Existing SBOM,
signed bundle and runtime formats remain unchanged. No network, new dependencies,
schema changes or CI activation are needed.
