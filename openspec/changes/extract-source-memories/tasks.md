# Tasks

- [x] Store-issued opt-in grants, queued revision pins, bounded reservations and receipts.
- [x] Data-only single-call worker with literal indexed paragraphs and no session impersonation.
- [x] Integrate ingestion, source withdrawal, exact forget suppression and extraction origins.
- [x] Current-account HTTP controls and collection/source React controls.
- [x] Add synthetic failure-focused Store/worker/HTTP/UI regressions.
- [x] Finish relevant Rust/React/DTO/format/clippy/OpenSpec verification and record actual results.
- [x] Independent review and schema14 integration after backup13, including final Linux and CRLF/mail evidence checks.

No live extraction-model evaluation or broad assistant-quality acceptance is claimed.

Final isolated macOS/Rust1.89 checks:10 extraction Store cases,17 affected knowledge
regressions, one scripted worker integration across success/forged fields/held
source-delete/wrong-model cases, one authenticated HTTP case and22 React cases
passed. Typecheck, production build, generated DTO comparison, affected-crate
all-target clippy with warnings denied, fmt, plan verification and27-item strict
OpenSpec validation passed. These are synthetic checks, not a live semantic score.

Integrated evidence: [automatic memory review](../../../docs/reviews/2026-11-13-automatic-memory.md). Initial392 Linux workspace cases and the final affected12 extraction/worker/installation checks passed. Final React31 includes the repaired request-ordering cases.
