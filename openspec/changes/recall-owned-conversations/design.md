# Conversation recall

Search is literal and owner scoped. It scans the newest4096 eligible earlier
successful runs and returns at most five role-labelled240-character snippet pairs.
The scope is current_thread or explicitly owned_conversations. Oversized turns are
omitted; truncated and scanned_limit prevent completeness claims. Read accepts one
actual search-discovered ID/revision for this consuming run; inherited revocation
pins cannot authorize a read. Whole user/assistant content is bounded to10KiB text
and14KiB encoded. Timestamps are actual admission Unix seconds.

A default-false private profile flag preserves stored-run capabilities. Foreground
admission enables it; missions and artifact-write stages exclude it. The generic
system availability text follows actual admitted tools.

Schema47 stores target/source/version and a discovery bit; it stores no text and
has no source FK. Every returned search candidate inherits knowledge, memory, mail,
calendar, artifact, learned and media withdrawal edges from the source. Context
inheritance propagates revocation edges without granting discovery. Guard checks
pins. Purge computes the learned/recall closure to a fixed point before clearing
outputs, plans, slots, checkpoints and stale events; source-thread deletion runs it
before source removal. Account deletion includes the dependency table. Artifact
writes reject runs that carry recalled private evidence.

This slice has no semantic/vector search, pagination, summaries, cross-account
access, private-history indexing into saved knowledge, or added model budget.
