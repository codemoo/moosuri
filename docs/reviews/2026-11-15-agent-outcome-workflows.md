# Agent outcome workflows — 2026-11-15

## Scope and observations

The existing authenticated HTTP/SQLite/Runtime runner now exercises four additional
synthetic workflows: chat remember/correct/recall across conversations and graceful
reopen; forgetting with transitive conversation evidence and unrelated source
preservation; cancellation during a blocked provider dispatch with replay/reopen;
and two distinct compound-calculation proofs followed by fresh-input reuse, proof
withdrawal and a new direct calculation without the withdrawn route.

The grader independently reads committed state, IDs/revisions, linked receipts,
arithmetic and scoped removal. A failed positive baseline remains a failed
validation and skips dependent mutants; absent proofs no longer panic the runner
or count as demonstrated rejection. Numeric normalization applies only to nested
receipt operands, preserving their multiplicity and the exact outer fresh inputs.

## Product defect found and fixed

The product's exact-number adoption gate rejected `23.2.` in a normal sentence.
It now accepts an otherwise exact numeric token with one terminal full stop.
Repeated dots, version suffixes, different signs and different decimal text remain
rejected. Scalar and workflow proof generation still verify the actual result and
receipt. An independent reviewer examined both callers and existing proof
revalidation and found no material defect in this narrow change. The original
sentence-shaped evaluation fixture was retained.

## Acceptance

The Linux actual CLI completed all ten worlds: seven completion-eligible outcomes,
one actual-mission outcome and two handling outcomes, with their separate
denominators. All 31 grader validation checks passed. The focused product
punctuation regression and setup-failure denominator regression passed. Native
sparse-proof and independent-decimal-oracle regressions also passed. Strict scoped
Clippy passed after factoring a timing selector type alias. An initial malformed
command-line argument was corrected without repeating the successful worlds.

Report format2 records 25 admitted-run timing samples, including missing values.
Two internal mission runs have no RunView timing sample; first-final-body and
other absent phases remain missing. These are loopback scripted-provider timings,
not model latency, browser rendering, load performance or semantic quality scores.
Graceful reopen is not abrupt process-kill recovery. Live models, comparative
skill efficiency, holdout and the broader original evaluation plan remain open.
