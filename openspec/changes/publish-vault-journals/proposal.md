# Opt-in off-host encrypted vault journal publication

P8/F51 needs independently recoverable journal coverage after a source failure. Extend the existing live capture and manually pinned historical recovery with a dedicated periodic worker, fixed pinned-SSH receiver command and independently queried immutable target ledger. The receiver retains only encrypted full transcripts and bounded pins; no plaintext journal proof files or recovery identities are required. No migration, dependency, CI, web, legacy or production change.
