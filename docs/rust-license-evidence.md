# Rust dependency notice evidence

Generate a deterministic notice report from the committed Cargo lock and an
explicit, already reviewed `cargo vendor --versioned-dirs` directory:

```sh
cargo run --locked -p xtask -- rust-notices --vendor /absolute/reviewed/vendor \
  > /absolute/output/rust-notices.json
```

The Rust command performs no download, package installation, source execution or
network access. It does not read a global Cargo cache. All crates.io dependencies
in Cargo.lock are included, including other targets and development tools. Local
workspace entries are listed separately; the project's own license files remain
part of the source distribution. This is not the minimal graph of a binary.

For each exact name/version the command compares the vendored package checksum to
Cargo.lock, verifies Cargo.toml and selected notice bytes against the local Cargo
checksum manifest, and preserves the declared `license`/`license-file` values. It
collects LICENSE/LICENCE/COPYING/COPYRIGHT/NOTICE basenames (including suffixes),
files under licenses/licences/legal directories and an explicit license-file.
Nested attribution files and all collected UTF-8 text are retained without
rewriting or replacing copyright holders. The report hashes the lock, package
manifest, metadata and every notice. Components and paths have deterministic order.

Sources must be a stable, trusted local vendor tree. Local checksum metadata is
not independently authenticated registry evidence; replacing both files and their
manifest can forge that evidence. This tool does not authenticate downloads or
provide a hostile-filesystem sandbox. It rejects observed symlinks, path traversal,
non-crates.io sources, mismatched metadata/hashes, non-UTF-8 notices and bounded
size violations. It accepts no arbitrary source path from a package except a
validated package-relative notice path. It emits no host paths, registry URLs,
timestamps, configuration or environment values.

Missing declarations or notices are explicit per-package fields and an aggregate
`components_missing_evidence`. They are never replaced with guessed license text.
To make missing evidence a packaging failure, add `--require-complete`: the report
is still emitted, then the command exits nonzero when evidence is missing. A zero
missing count means only that a declaration and candidate file were found for every
registry component. It does not establish that every attribution, bundled source
license, dual-license choice or source-distribution obligation is satisfied.

Keep the generated report with release preparation artifacts, not a stale copy in
Git. Review exact upstream evidence for missing files, embedded third-party code,
license choices and source obligations. npm/embedded React licenses and assets need
their own coverage; this command does not replace that work. The report is not
automatically inserted into a signed bundle or its existing provenance format.
The [source inventory](dependency-inventory.md) and
[binary provenance](binary-provenance.md) retain their separate purposes.

The current Linux check collected784 candidate notices (4,085,403 raw text bytes)
for434 registry packages. Twenty-one packages had no selected notice file in the
supplied vendor archive, so complete release notice acceptance remains open.
[Evidence](reviews/2026-11-15-rust-license-evidence.md) records that exact scope.

Use optional [`--supplement`](notice-supplements.md) to attach exact external
source notice text to matching lock identities. This retains original local
missing fields and strict failure. The bounded demonstration adds separate
evidence for17 of21 missing Rust components without altering vendor checksums.
