# F22/P3 fixed Korean search aliases — 2026-11-15

## Scope and verification status

R0 adds a real optional `korean_aliases_v1` search mode across Store/API/native tool/React. It expands only three immutable public Korean word families with at most three complete normalized query variants. Default absent/null/literal mode omits its serialized field; alias mode/version remains bound to cursor identity. Exact collection/subject/speaker and existing time/owner/deletion filters do not expand. FTS OR membership unions matches before one stable ID page, avoiding duplicated overlapping aliases.

Whole normalized family queries expand; otherwise exactly one raw whitespace word must match a family. Other query words/order remain. Multiple matching words, unknown/inflected/punctuated family words fall back literal. Ordinary short-substring queries retain the existing path. Alias-mode source learning and evidence bindings are refused, including fallbacks, because current reusable routes do not bind a structural match mode. Literal routes remain compatible and generation3 is unchanged. No migration/dependency/provider or production changes were made.

R0 was frozen before Rust execution; Linux was the first Rust validation target,
without waiting for or repeating a native build. The initially provisional API
projection has now been replaced with actual built Linux exporter output. The
integrated acceptance below records the executed checks. The private source-time
baseline commit was not published; only the feature delta was integrated.

## Fixed hand-authored comparison

The public `crates/moosuri-store/src/knowledge/fixtures/korean_aliases_v1.json` was written before implementation. It declares18 source IDs and12 query IDs, each with explicit expected literal/alias result IDs. The implementation does not generate those expectations. Cases cover all three families, complete phrases with the family at different positions, whole-query whitespace normalization, unrelated/inflected/punctuated terms, inserted phrase words, two different families and two words in one family.

The actual Linux Store search matched all12 independently written per-query sets:
23 literal hits and44 alias-mode hits across the fixed query/source pairs. The gold
was unchanged after implementation. These are bounded lexical result counts, not
a natural-language quality score, holdout result or broad recall claim.

## Executed Linux Rust checks

- `cargo test --locked -p moosuri-store korean_aliases_`:3 passed. Actual Store search compares both modes with all12 fixed ID sets; typed alias successes decline source learning while ordinary independent literal proofs become ready; an explicit alias-mode literal fallback cannot supply an evidence-calculation binding.
- `cargo test --locked -p moosuri-agent korean_aliases_`:1 passed. Omitted and explicit literal modes preserve the old JSON projection, alias mode survives serialization and an unknown version rejects.
- `cargo test --locked -p moosuri-server --test http_boundary korean_aliases_`:1 passed.23 source records produce21 observation-eligible alias matches across20+1 pages, including one source containing all aliases that must appear once. Mode/text/range changes cannot reuse the cursor. Exact speaker and collection scope, validity, another account, deletion and generation mutation remain fenced.
- `cargo clippy --locked -p moosuri-core -p moosuri-store -p moosuri-agent -p moosuri-server --all-targets -- -D warnings`:passed.
- `moosuri export-types` (actual built Linux binary):the built binary output exactly matched the provisional projection after removing its marker; actual output replaced that file.

The filters above intentionally avoid broad unaffected suites and the1001-record cap suite. No real model/provider/device operations are required by them.

## Checks actually completed

- `npm run typecheck`:passed against the marked provisional type delta.
- `npm test -- --run src/Knowledge.test.tsx`:21 passed, including omitted default mode, explicit opt-in, invalidated prior page and exact alias-mode continuation.
- `npm run build`:passed.
- Standalone cached `rustfmt --check --edition2024 --config skip_children=true` on the14 changed Rust files:passed, without a Cargo process.
- `git diff --check`:passed.
- `openspec validate add-korean-search-aliases --strict`:passed.

Frozen manifests record exact changed source, gold and evidence hashes. Local node_modules symlink and generated dist output are excluded. Common README/status/features/testing files are root-owned and were not edited.

## Independent review and integration

Independent frozen review found one compile blocker: the learned-source eligibility
check referred to an enum absent from that module's imports. The integration owner
qualified it with `moosuri_core::KnowledgeMatchMode` before the first Rust run. No
other material semantic/cursor/boundary issue was reported. The already published
dated-memory search helper received an absent mode field when joining the changes.

All five offline Rust1.89 Linux phases passed: Store3, parser1, HTTP1, four-package
all-target Clippy, and the embedded assistant build. Source hashes matched after
execution; source archive SHA256 was
`91bddf7d1cf2b6fe6f79fff66db93c206d9e0d599342e5f41dce87ec8aa42966`.
The actual generated declaration SHA256 is
`c64fb16bc678d8cc71cac57cb503779866654fbd7a009ce53e376d43f5a2bb91`.
The production React build also passed; its21 unchanged UI cases were not repeated.
No native Rust rerun, broad regression suite, provider call, CI change or production
deployment was added.
