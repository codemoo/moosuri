# Design

The existing private temporary IndexedDB handoff stores bounded ArrayBuffers beside
text. The public Cache API remains public-only. A shared input has at most4 files,
32MiB total and1 audio file, with type/name and per-kind limits before staging. A
32MiB+32KiB streaming body limit and8s deadline precede multipart parsing. The5min
absolute TTL and atomic one-window claim/consume protocol remain unchanged.

Review renders local image/audio object URLs and removes them on removal, retirement
or unmount. Explicit insertion preflights existing draft capacity and active recording;
it consumes once, then rechecks authority and current composer state before delivery.
Photos/documents enter existing media preparation/upload/retry/cancel. Audio enters
local voice preview and requires explicit conversion with the existing immutable
speech attempt. It never requests the microphone or automatically calls a model.

Browser codecs determine available audio formats. The120s/32MiB decoded sample checks
occur after decode and are not a hard decoder allocation sandbox. Temporary files are
plaintext in the trusted browser profile; expiration clears on access, not guaranteed
physical disk erasure at a wall-clock deadline. A crash after consume can lose input.
