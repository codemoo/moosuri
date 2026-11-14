# Source-lock SBOM acceptance

The offline Rust `xtask sbom` reads only the four committed Cargo/web/vault-web/
OpenSpec locks and writes deterministic CycloneDX1.6 JSON. It does not invoke a
package manager, resolve/download a dependency or read private configuration.

Linux x86_64 regression covers distinct Rust versions, scoped and nested npm
occurrences, omitted private resolved addresses, unsupported Rust source rejection,
traversal rejection and stable results. Actual repository output contains642
components. It passed the official [CycloneDX1.6 schema](https://github.com/CycloneDX/specification/blob/master/schema/bom-1.6.schema.json)
using JSON Schema Draft7 validation. The schema bytes used have SHA-256
`2971bd4824ed5e81a0c0bd42f66a0dc94e256075071c799e2aa5893c44233a52`.
Only the validator's temporary development environment uses Python; the committed
generator is Rust and the product gains no Python/Node runtime.

Focused test, Clippy with warnings denied, actual generator and plan verifier
passed. A first Clippy pass requested a collapsed conditional; only that equivalent
expression changed before final Clippy/generator checks. No unrelated product suite
was repeated. The source-lock scope includes build/dev/other-target entries and
does not prove a release binary's composition, license clearance or vulnerabilities.
No generated SBOM, downloaded schema or temporary validator package is committed.
