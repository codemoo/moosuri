# Numeric run timing observations —2026-11-14

P0/F54 adds actual content-free observations through existing Store/Runtime paths.
See [metric definitions and limits](../run-timings.md). No model/effort/default,
authority, schema, dependency, telemetry service or execution budget is changed.

A private numeric extension in existing run model JSON records original attempts.
Both claim paths record queue/history assembly, durable step receipt records dispatch
intent, common Runtime stream/tool handlers record observed intervals, and guarded
output persistence links first body arrival marks to surviving final-phase output.
Only a completed explicit final nonrefusal body qualifies; unphased, commentary,
tool-only, partial and failed output is not a successful final-body timing sample.
Aborted/unobserved ends stay null. Common terminal hooks and current-state outcome
fallback retain failed/cancelled/unknown denominators without fabricating direct SQL
or restart durations. Existing reported usage and original retry counters stay intact.

Owner RunView diagnostics and existing owner/admin UsageSummary add latest128-retained-
run aggregates, fixed outcome keys, original-attempt/sample/missing denominators and
nearest-rank p50/p95. No text/prompt/tool payload/name, URL, credential or private ID
is exported by timing DTOs. Existing admin usage account labels retain their current
permissions. Timing JSON disappears when run/thread rows are deleted; source withdrawal or
output purge can retain those rows and their numeric/accounting observations. Old
token receipts retain their separate existing all-time scope. No browser/render/network-roundtrip claim.

Focused synthetic verification:

- `cargo test --locked -p moosuri-store -p moosuri-agent run_timings -- --test-threads=1`:Runtime1/1 and Store1/1 passed. Runtime covers successful unphased normal streaming, actual two-round local tool timing, truncated stream and cancellation. Store covers commentary versus explicit final phase promotion, original retry attempts, reopen discontinuity, legacy absence, owner isolation/admin denominators and numeric-only aggregate content exclusion.
- `npm --prefix web test -- UsageView.test.tsx`:1/1; missing first-body samples, fixed failure/unknown denominators and legacy absence display.
- `npm --prefix web run typecheck`:main/worker passed. New optional fields preserve old response fixtures.
- Pinned OpenSpec2.0.0 strict `observe-run-latency` validation, rustfmt and diff whitespace passed.

Initial Runtime fixture incorrectly attempted an OpenAi loopback provider override;
existing fixed-origin validation rejected it. The fixture was corrected to the real
Compatible Runtime path rather than changing production origin policy. Explicit
phase behavior is covered through the real Store output APIs. No failed fixture was
counted as a performance or quality result.

No real provider/model inference, personal account/history/credentials, external
messages, legacy service/cron/data, CI, commit/push or subagents were used. This is
collection correctness from a few synthetic outcomes, not a benchmark/quality score.
Canonical integrated type export, independent frozen-source review and Linux/build/
Clippy gates belong to root. Native shared lane was released after the focused filters.
