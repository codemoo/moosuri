- [x] Add bounded owner-scoped typed history discovery and whole-turn read.
- [x] Pin new foreground admission without expanding old or background profiles.
- [x] Track every returned candidate and inherited upstream/context provenance.
- [x] Expand destructive learned/recall closure; block recalled artifact publication.
- [x] Verify focused Store4/4, actual Runtime/old-profile2/2 and affected existing knowledge chain1/1; exact DTO export, web typecheck and strict OpenSpec validation.
- [x] Complete source-pinned independent review and parent integration validation.

### Full-window search measurement

The opt-in `full_window_namesake_search_timing` fixture submits 4,096 synthetic
completed turns on one account. Every turn matches the Korean lookup phrase.
The oldest turn is the only one for the requested club. The test opens an
immediate SQLite transaction for each search, checks that the oldest result
ranks first and that there are more than five eligible results, then rolls back
the discovery pins. It records elapsed time without imposing a clock threshold.

On macOS arm64, Rust 1.89.0 in the unoptimized test profile, three searches on
2026-11-20 took 563.25, 558.01 and 565.87 ms at `bd1a6ce`. The same fixture took
116.56, 106.19 and 101.01 ms with the lightweight text scoring pass. Fixture
creation is excluded. This measures one local workload with valid matching
turns. Large groups of otherwise competitive but withdrawn or unreadable turns
can still require eligibility checks until five valid results and overflow are established.

```sh
cargo test --locked -p moosuri-store --lib full_window_namesake_search_timing -- --ignored --nocapture
```
