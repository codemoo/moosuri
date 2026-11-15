# Download explicitly pinned signed bundles

P10 already authenticates and installs local bundles but lacks a bounded transport from an
explicit operator mirror. Add a Rust CLI download route using the existing Ed25519 pin and
local verification contract. Authenticate the manifest before parsing it or requesting large
files; publish only a complete verified bundle to a new owner-private path, without execution.

This bounded slice supports direct versioned HTTPS mirrors and an optional explicit private
CA. It does not establish official mirrors/keys/artifacts, discover a latest release, install
services, change local install/update semantics or close the whole P10/public-release gate.
References: P10 in docs/plan.md, installation requirements in docs/features.md and
local installer acceptance in docs/local-installation.md.
