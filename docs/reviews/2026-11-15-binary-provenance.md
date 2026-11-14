# Offline binary provenance evidence

Scope: xtask packaging/verification, shared strict evidence codec, optional signed
manifest binding, installation staging/retained-slot verification, same-schema
manager compatibility and conservative evidence uninstall. No Store schema,
dependencies, CI, service changes, private data or actual signing keys are involved.
Synthetic tests use the existing deterministic fixture Ed25519 key and harmless
synthetic executable probe, not an official release or authenticated native build.

Native Rust1.89 focused command:

`cargo test --locked -p xtask -p moosuri --bins release_provenance -- --nocapture`

Passed5: app3 and xtask2 (the shared codec test compiles/runs in both). Checks cover
actual packaging from current source-lock bytes and declared local Git/compiler
metadata, inventory mutation/refusal to overwrite, strict fixed paths and target
binding, legacy no-provenance compatibility, inventory rejection before prefix
creation, immutable captured source bytes, installed slot revalidation, same-schema
manager refresh through activation and complete conservative evidence removal.

Scoped `cargo clippy --locked -p xtask -p moosuri --bins -- -D warnings` passed.
Its initial failures were two boolean-style diagnostics and one needless borrow;
equivalent style repairs passed without repeating unchanged behavior tests. They
were lint failures, not missing embedded assets. No release-build acceptance was
claimed. Final source hashes are recorded in the freeze handoff. Main owns
independent integrated Linux acceptance; full native official release, clean build,
distribution/signing and builder authenticity remain open. No repeated full app
suite or production/provider acceptance was performed.


## Independent repair and Linux acceptance

Independent review found that an explicitly signed `provenance: null` could be
accepted as legacy while an old strict manager rejected the new field. The R1
field deserializer now accepts absence or a valid reference and rejects explicit
null before any installation mutation. The signed pre-mutation regression covers
that distinction while preserving absent-field legacy compatibility. Root reviewed
the production delta against the original finding.

Linux R0 passed the three app checks and shared-codec check; its packaging fixture
failed because the public source capsule intentionally has no Git history. R1
creates an isolated deterministic synthetic Git repository with literal source
locks. Product metadata/dirty checks were not relaxed and no real Git history was
copied. The exact packaging and signed-null tests then passed, followed by strict
scoped Clippy and the joined default Linux application/xtask build. Previously
passed unaffected tests were not repeated. Synthetic source metadata, artifact
bytes and signing pins remain distinct from official release acceptance.
