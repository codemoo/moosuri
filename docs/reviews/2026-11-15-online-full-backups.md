# Online full-data export acceptance

The optional serve worker captures the live SQLite database and fully applied
auth/deletion journals, then reuses the existing stopped-export format2 encryption
and no-overwrite publication after releasing the writer. No schema or dependency
changes. The operator must already configure the independent recovery recipient.

Focused synthetic checks exercise actual Store capture, live mutation before
encryption, immutable snapshot inspection, latest-journal receiver and historical
restore, damaged/missing journals and duplicate output rejection. A worker test
uses an initialized Store and real filesystem encryption, verifies the first
snapshot, shutdown without a second attempt, and continuing Store access. The
existing stopped-format acceptance covers the factored common code.

Final source passed Linux Store2, periodic worker1 and unchanged stopped-format1
checks, plus scoped strict Clippy and the embedded-web binary build. The snapshot
world executes actual receiver publication and historical restore; it proves the
later forgotten memory is absent while retained snapshot data remains. Product
source hashes match the executed snapshot. A missing empty auth journal initially
exposed the generic journal parser's initialization path; the live stage now
requires all four bounded private regular input files before any parser opens.
The failure was repaired and the focused checks passed.

An independent reviewer verified the frozen Rust hashes and accepted capture,
shared codec, output and shutdown boundaries without a material finding. No live
account, production data, extra approval flow or CI was used. This is not a
production backup, actual off-host setup or power-loss trial.
