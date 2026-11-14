# Normalized call feed — 2026-11-15

The once-only Rust producer imports selected normalized call transcripts into one
explicit per-owner custom source connection. Optional fixed-provider canonical WAV
transcription covers up to20minutes in at-most120second chunks. Literal complete
transcripts, settled file hashes, original ingest receipts and source revisions
provide the evidence; original media stays local. Unknown/mixed speakers carry an
explicit reserved marker and cannot become automatic memories in either reported
or collection-specified attribution mode.

## Review and repair

Independent R0 review verified the frozen21-file source and found no additional
material owner/path, complete-publication, receipt-recovery or attribution issue.
It confirmed a product defect: definite pre-dispatch errors and401/403/429 rejection
were treated as permanently unknown. R1 persists compact first/last rejection
identities, reasons and count. A later explicit invocation may create one fresh
intent. A scan-local attempted-chunk set prevents duplicated descriptors from
retrying in that same invocation. Unknown transport and invalid/incomplete response
states retain their original durable no-resend fence. Root reviewed this repair,
its error classification, bounded history and default-compatible saved state.

## Focused acceptance

Native R0 passed the actual Store/router call workflow, both attribution modes,
format/WAV boundaries, affected shared Codex producer cases and strict scoped
Clippy. Linux R0 passed actual ingest1, attribution1, format2 and one representative
shared-producer receipt-recovery case, then Clippy and default binary build.

Linux R1 reran the one changed actual workflow: an explicit401 rejection, key-file
repair and later invocation completed both audio chunks; two same-audio descriptors
produced one bad-key request; the original rejected UUID remained attached to the
new successful receipt; the genuinely unknown chunk was never resent. The same
fixture retains long-audio complete-only publication, source revision/dedup,
settled-file deferral and lost ingest-ACK recovery. Scoped Clippy and the joined
default Linux application build passed. Unchanged format/attribution/shared tests
were not repeated after R1. No schema, dependency, web DTO or CI change was needed.

All audio, keys, users, server data and HTTP responses are synthetic. These results
are not actual transcription quality, provider/account/rate acceptance, mobile or
vendor-format coverage. Per-state64chunk/2MiB recovery limits are explicit; no
receipt eviction, partial-success truncation or automatic raw-audio archive is
claimed. [Usage and complete limits](../call-feed.md).
