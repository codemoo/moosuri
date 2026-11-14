## Why

The existing standalone recovery path permanently seals a stopped source, which
cannot serve as routine password-vault backup. Add a coherent non-sealing path
without weakening that exact cutover contract or claiming independent latestness.

## What changes

Opt-in service flags run a sequential local age snapshot worker. Capture uses the
same Database mutex as HTTP, includes SQLite WAL and exact journal, then verifies
and encrypts outside the writer. A separate purpose/magic and explicit restore
command preserve old format behavior. Restore quarantines sessions/pending
authority and assigns a fresh service identity. Newer independently pinned journal
publication/forward recovery remains a separate contract, not a claimed feature.
