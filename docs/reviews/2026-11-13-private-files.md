# Private UTF-8 file attachments — 2026-11-13

Schema27 adds immutable TXT, Markdown, CSV and JSON originals up to24KiB each.
Independent uploads, progress/cancel/retry, composer/message/library previews and
exact original download share the existing account, deletion and recovery fences.
The actual model protocol receives the selected revision, SHA256, byte interval
and complete text as untrusted data. The current question and selected sources
survive whole-oldest-turn context trimming. No parser or model quality is inferred
from a filename; PDF, audio/STT, Office and general binary files remain unsupported.

Independent review found PF-01: an old upload explicitly selected again could be
evicted from follow-up context because selection used upload age. The repair uses
the latest explicit run sequence within the same tenant, account, conversation
and current-run cutoff. Retry and unrelated/future-run adversaries passed. The
repair and Calendar26-to-files27 integration were independently accepted.

## Verification

- macOS: Store319 passed with2 opt-in checks ignored; file HTTP2 and image HTTP1;
  React268 across29 files; exact generated DTO comparison, CLI19, workspace
  all-target clippy, formatting, plan links, types and production build passed.
  The Store subprocess helper is nested in the total, not an extra test.
- Linux x86_64: hash-verified source and embedded web passed files9, media13,
  restore9, schema8, agent62, server46 and CLI19. These166 phase passes have
  overlapping filters;3 opt-in cases were ignored. The final toolbar bundle also
  passed CLI19. The later one-line row-layout CSS change was rebuilt and checked
  in the actual macOS browser; the Linux bundle predates only that CSS change.
- Actual Chrome390×844, Rust HTTP service and synthetic loopback model: a held
  upload did not block sending text, and the first created conversation retained
  the pending upload. File previews escaped script-looking text, and download
  returned the exact70 original bytes. A failed model round retried the same run
  and file without overwriting a new draft; a follow-up received the exact source.
- Restart retained the authenticated session and original. Deleting a file
  withdrew derived replies and made its original return404, while preserving the
  browser draft. The loopback verified byte/digest evidence, untrusted-data
  instructions and absent tools, and derived quantity37 from the supplied JSON.
- Final UI changes replaced oversized native file chooser presentation with
  accessible compact pickers, removed duplicate filename/download labels, and
  kept previews scrollable. Related React39, types/build and the rebuilt embedded
  browser passed. Final pickers and controls were44px high, the document did not
  overflow390px, and the final browser console had no errors.

The integrated independent review covered the source and deletion/recovery
boundaries before the final presentation-only toolbar changes. Root reviewed and
rendered those changes separately. Only synthetic documents and accounts were
used. No actual private file, paid model, Google account, phone or production
service was used. Generated model citations, general file understanding and the
entire media roadmap are not claimed as validated by these checks. See the
[supported contract](../private-files.md).
