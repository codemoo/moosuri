# Knowledge HTTP fixture alignment

2026-11-14. Test-only correction; production behavior and permissions are unchanged.

The existing question `What does my Friday note say?` reaches the shipped isolated
Calendar/mission decision round because it contains a temporal cue. The scripted
HTTP provider previously expected ordinary read tools immediately and panicked.
The fixture now returns a strict `no_command` from that isolated round, then checks
the original source search/read/answer path. Its search arguments include the
current nullable cursor field. It checks both classifier calls and all eight
dispatched calls across two turns; deletion still retains usage accounting.

The original failure remains in private validation evidence. The repaired native
HTTP test passed, and the integrated Linux HTTP suite passed 33 tests with one
explicit public-network test ignored. Source deletion still clears history/SSE
replay and prevents a held late answer from republishing private source text.

These are synthetic Rust/SQLite/loopback tests. They do not demonstrate language
understanding, live-provider quality or reduced classifier latency.
