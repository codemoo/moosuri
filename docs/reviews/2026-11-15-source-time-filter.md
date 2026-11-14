# F22/P3 source observation-time filter — 2026-11-15

## Result and scope

Source search now accepts explicit `observed_from` inclusive and `observed_until` exclusive bounds across the authenticated search endpoint, native source-read tool and existing React knowledge search. It reads the current source revision's existing `observed_at`; assertion validity remains a separate predicate. Nonnull observation bounds on memory search are rejected, and the existing closed source/memory kind does not admit a mixed query. Supported bound timestamps span year1–9999; equal/reversed intervals are rejected. No SQL migration, dependency or capability-generation change is needed.

Both fields default to absent and omit absent values on serialization. The old unfiltered serialized query, receipt payload and cursor scope remain identical. A supplied range is included in the ordinary query-bound cursor hash with every other query field, owner and knowledge generation. The interval filters before FTS matching and before the short-substring bounded candidate enumeration. Source timestamps remain owner/producer reports, not independently verified event times. Sources require a typed nonnull observation timestamp; an incomplete ingest request is rejected rather than using now or validity as a substitute.

Current reusable source routes have no structural date parameter. Date-bound searches therefore cannot create source-learning proofs or evidence-calculation bindings, while unfiltered routes/proofs retain their old arguments. This refuses unsupported learning scope instead of silently exporting a private fixed date. The native tool only exposes optional read arguments, and accepts old wire calls that omit them. Existing authentication, source/current-version/deletion and collection scope predicates remain.

## Native and HTTP evidence

Existing cached Rust1.89.0 aarch64 Darwin tools were used serially. No model/provider credentials, private data, services, production databases, installs or CI calls were used.

- `cargo test --locked -p moosuri-server --test http_boundary source_observation_`: **1 passed**. The actual authenticated HTTP/router/Store path ingests23 sources with identical assertion validity and different observations. It verifies `[200,300)` includes200 and299 but excludes199 and300, both open-ended intervals,20+1 pagination, exact-range cursor conflict, bound/order validation, memory/mixed refusal, independent validity exclusion and both FTS/short-substring paths. Missing `observed_at` ingestion is rejected. Another real account sees no owner sources and cannot reuse its cursor. Actual source deletion invalidates the old page and excludes the source from fresh results. These are Store outcomes exercised through HTTP, not a separate second Store test count.
- `cargo test --locked -p moosuri-store source_observation_`: **1 passed**. The real typed search/read/finalization path completes two independent date-bound reads without creating a learned route, then two unfiltered reads create the ordinary ready source route. The fixture observation is100 and bounds99..101.
- `cargo test --locked -p moosuri-agent source_observation_`: **1 passed**. The native parser preserves both interval arguments separately from validity. Old omitted-field input round-trips to the identical JSON projection, preserving legacy query/cursor/proof serialization.
- `cargo clippy --locked -p moosuri-core -p moosuri-store -p moosuri-agent -p moosuri-server --all-targets -- -D warnings`: passed. Existing nonfatal ts-rs unsupported-serde diagnostics remain.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The HTTP fixture corrections during development were an initial20-vs21 page count, explicit i64 test literals and the existing source-delete expected-revision body. The learning fixture initially used a period outside its seeded observation100. These were fixture issues; final evidence logs contain successful outcomes.

## React and contract evidence

- `npm run typecheck`: passed.
- `npm test -- --run src/Knowledge.test.tsx`: **20 passed**, including local-time observation fields, independent validity, exact continuation payload and invalid reversed-range input retention without a request.
- `npm run build`: passed.
- `openspec validate filter-source-observation-time --strict`: passed.

Shared API types were emitted by the actual compiled Rust exporter. The local node_modules symlink and dist output are excluded from the patch. No common README/status/features/testing files were edited.

Independent frozen review accepted R0 with no material findings. The integration
owner verified19 files against their exact frozen hashes and merged the existing
dated-memory test file, including absent observation fields in its newly added
as-of search helper. The integrated production React build passed; the unchanged
20-test UI evidence was retained.

The exact integrated offline Rust1.89 Linux source passed HTTP1, learning1, parser1,
and the six dated-memory outcomes whose search helper was joined. Four-package
all-target Clippy and the embedded CLI build passed. Source hashes matched after
all six phases; source archive SHA256 was
`fb90cbb196da64d8de7203ed5b06abc98ed4791437ca83590e80d90af3388999`.
The actual built Linux exporter exactly matched the shared TypeScript declarations.

The new evidence-calculation bound refusal was reviewed in code; it was not a
separate executed outcome. These checks require no real account or provider calls
and do not certify natural-language date understanding, synonym retrieval or every
F22 requirement. No CI or production configuration changed.
