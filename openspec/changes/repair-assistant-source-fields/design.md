# Design

The saved-source binder continues to locate exactly one literal label. It trims
ASCII horizontal whitespace, optionally consumes one `:`, `：` or `=`, then trims
horizontal whitespace again. A label already ending in a separator cannot consume
another. It does not cross lines, skip words, convert units or extract a partial
decimal. Existing checked arithmetic and complete source/revision verification
are unchanged, and the stored offsets still identify only the observed number.

Media guidance permits labelled PDF extraction already supplied by the server.
It does not imply OCR, visual layout understanding or access to unextracted bytes.
The source remains untrusted and all tools/persistent effects remain unavailable
in the current media route.
