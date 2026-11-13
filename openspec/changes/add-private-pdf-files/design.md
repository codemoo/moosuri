## Process and source identity

The deployment-owned current Moosuri executable handles a hidden PDF-only CLI branch before any runtime/data directory initialization. Server-side code supplies only that constant command and stdin bytes; it clears inherited environment, uses root cwd, closes private output to logs, bounds stdout, enforces CPU/wall/output/descriptor limits and Linux512MiB address space. Timeout/error/drop kills the child. This is resource isolation, not a different-UID sandbox. The parser uses strict loading and bounded decompression; no password, filesystem/URL fetching, active script, rendering or attachment execution is implemented.

Original PDF bytes and their exact admitted digest remain immutable. Existing SQLite columns store MIME and extracted text. Text is page-marked and bounded before storage; source byte offsets always refer to extracted UTF-8, never to compressed PDF bytes. Empty/scanned or partial/over-limit extraction does not become a ready source. Parser caps supplement rather than replace process bounds.

## Ownership and UI

The existing authenticated file lease, original request UUID/name/digest, quota and tombstones gate settlement. Deletion/session loss/epoch changes cannot admit late parser output. Retry preserves original bytes; restoring a pending file requires reselecting that source. Original download is authenticated/no-store/attachment, text preview is escaped, and the PWA never caches it. PDF upload preparation does not block sending current text or independent image work. Error codes are a fixed allowlist, never parser/provider prose. Attachment-derived context keeps existing no-tools/no-persistent-writes policy.

## Verification

Use real synthetic PDF parsing (ordered pages, ToUnicode Korean, malformed/empty/page/text/byte limits), actual compiled CLI parent/child protocol with no installation initialization, Store digest/session/tenant/delete/restart, actual HTTP/browser upload-preview-download and PDF retry, Linux subprocess bounds, and independent parser/resource/storage boundary review. UI mock tests are not a PDF parser or mobile-device score. No live model quality claim.
