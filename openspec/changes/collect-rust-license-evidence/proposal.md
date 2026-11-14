# Collect Rust dependency license evidence

P10 release preparation has a source-lock inventory but no tool to collect the
actual dependency notices. Add an offline Rust command that preserves notices from
explicit vendored sources, validates the lock/local checksum evidence and reports
missing files without guessing legal terms or declaring release clearance.
