# Offline notice supplements

`rust-notices` and `web-notices` accept an optional `--supplement ABSOLUTE_MANIFEST` to retain explicitly reviewed external notice text alongside their existing local evidence. The shared reader performs no network access, dependency resolution, installation, package scripts, cache discovery, or modification of source/checksum manifests. It does not authenticate a repository, release commit, registry archive, signature or legal obligation.

```text
cargo run --locked -p xtask -- rust-notices --vendor /absolute/reviewed/vendor --supplement /absolute/reviewed/rust-supplement.toml --require-complete
cargo run --locked -p xtask -- web-notices --web-node-modules /absolute/reviewed/web/node_modules --vault-node-modules /absolute/reviewed/vault-web/node_modules --supplement /absolute/reviewed/web-supplement.toml --require-complete
```

Each manifest contains one ecosystem. `cargo` entries match exact name/version and `registry_package_sha256`; `npm` entries match exact name/version and `lock_integrity` SRI. npm aliases and repeated app/installation positions receive the supplement only when all three values match. Every supplied entry must match at least one current lock component; unknown identities and wrong digests fail rather than being silently ignored.

## Manifest

Use TOML format1 with the fields shown below. This synthetic example is not a notice for a real dependency and will not match the project's lock. Replace identity/digest/source fields using reviewed public evidence; never insert a guessed SPDX template.

```toml
format = 1
[[entries]]
ecosystem = "cargo"
name = "demo"
version = "1.0.0"
lock_digest = "abababababababababababababababababababababababababababababababab"
repository = "https://example.test/reviewed/demo"
commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
package_path = "crate"
provenance = "reviewed_commit_only"
[[entries.notices]]
path = "NOTICE"
sha256 = "d290523526ff1f5d83417f21c5d8ac996b0eecd7f29710211779231bc6f05346"
text = "Synthetic notice only.\n"
```

Repository is an explicit HTTPS repository coordinate without userinfo/query/fragment. Commit is a full40-character lowercase hexadecimal ID. `package_path` records a known source package subpath; empty means no narrower package subpath is asserted. Each notice has a repository-relative `path`, exact UTF-8 `text` and lowercase SHA256 of those decoded UTF-8 bytes. No newline, attribution or copyright rewriting occurs. `path` is provenance metadata, never a host file selector. For an exact source-header fragment, optional `lines = [1, 19]` records the inclusive source line range as an operator assertion; its hash still covers the supplied fragment, not the whole source file. This tool does not fetch or prove that assertion.

`provenance` is one of `registry_vcs_view_unverified`, `registry_git_head_unverified`, `attestation_payload_unverified`, or `reviewed_commit_only`. They distinguish the operator's evidence route; none means the tool authenticated the release-to-source mapping. Every output supplement explicitly has `authenticity_verified: false`. Unknown fields, duplicate TOML keys, duplicate exact identity/digest entries and duplicate notice paths are rejected. One reviewed source coordinate per exact identity/digest is supported; this does not model or resolve all embedded-code obligations.

The manifest must be an absolute real regular file without symlink ancestors, traversal, empty path segments or trailing separators. Source metadata paths reject traversal/backslashes/control characters. Inputs are bounded to8MiB per manifest,512 entries,512 notices per entry,4096 notices total,2MiB per notice,8MiB supplied notice text and64MiB notice text after expansion across matching positions. Non-UTF-8/empty/NUL text and wrong hashes fail closed. Use stable, trusted sources: observed symlink checks are not a hostile concurrent-filesystem sandbox. No external notice files are opened; all selected text is inline.

## Separate coverage, unchanged strict gate

Without `--supplement`, existing report output and gate semantics remain unchanged. With it, matching components gain `supplemental_evidence`; the top-level `supplement` records the manifest hash, supplied entry/notice bytes, expanded bytes and matching/incomplete-position coverage. Evidence has deterministic source-path order. The report contains no supplied host manifest path, timestamps or environment values.

Original declarations, notice text, missing metadata/declaration/notice flags, referenced-notice gaps, conflicts, `evidence_complete`, and aggregate missing counts are preserved exactly. A supplementary notice does not repair those local flags or make `--require-complete` succeed. Strict mode still emits the complete report and then fails for the original missing evidence. Existing declaration conflicts are never normalized or resolved. A locally complete report remains only local candidate coverage, not legal clearance.

The real bounded demonstration attached27 source notices to17 Rust versions and3 notices from2 npm identities to4 app positions. Original local gaps remain21 Rust /66 web positions; both strict commands exit1 after emitting reports. Four Rust versions have no supplement: `io_tee@0.1.1`, `intl_pluralrules@7.0.2` and the two `winapi-*-pc-windows-gnu@0.4.0` packages. The differing pluralrules declaration/README statements are not resolved. Registry archive binding, unverified attestation signatures, platform/bundled notices, license choices, source obligations and final release/legal review remain open.

Generated manifests/reports and collected public notice text stay in private release-preparation storage, not Git. Only implementation, synthetic tests and this contract are committed. This optional input neither embeds notices into a signed release nor changes the existing release/provenance format. See [Rust evidence](rust-license-evidence.md), [web evidence](web-license-evidence.md) and the [dated bounded record](reviews/2026-11-15-notice-supplements.md).
