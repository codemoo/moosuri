# Direct destructive recovery intents

Coverage begins at migration0015. The local writer retains MSAUTH01 authentication
recovery unchanged and freezes the exact terminal MSKNOW01 knowledge prefix.
Pending legacy records replay before successor effects and before readiness.
New knowledge forget/source delete/collection disconnect/source revocation,
command forget, thread delete and mail disconnect adopt through MSDSTR01 in
`destructive-intents.journal`. No operation appends both knowledge formats.

The successor header binds the installation UUID and frozen predecessor
sequence/head. Migration creates and fsyncs the private header and parent directory
before adopting its schema metadata. An interrupted header is reused verbatim;
a partial, nonempty pre-adoption, missing installed, wrong-installation or mismatched
predecessor journal fails closed. It is never truncated or silently reset.

After authority and revision checks, one append plus fsync adopts the complete
operation. The actor stays poisoned until a SQLite transaction commits the full
scoped effect, dependent plaintext/index/run purge, any command/page receipt and
the successor watermark. A post-append failure blocks all actor work until reopen;
startup applies the verified pending prefix transactionally before readiness.
Checksums detect accidental damage against the DB prefix; they do not authenticate
a writer or protect against replacement of both DB and journals by the OS owner.

Records contain opaque tenant/object UUIDs and bounded revisions or epochs. A
command forget includes the direct memory UUID, head/effective revisions and
command UUID. Normal replay validates/finalizes its existing sealed command slot;
it never uses that slot to discover the durable target. A future historical
replayer can use the direct target when a snapshot predates the slot; no such
restore/import interface is implemented here. Source batches use one record with
at most32 distinct direct source UUIDs. Source text, remote IDs, credentials,
quotes and model text do not enter these records.

A mixed source page validates all upserts before adoption. A crash after its
single tombstone record but before the combined DB commit withdraws those sources
on replay without inventing a page receipt or advancing its cursor. The producer
queries the business request UUID first. If no receipt exists, it obtains a fresh
server lease and retries the same entries/tombstones/expected sequence/checkpoints
and request UUID with the new run/generation. The old lease remains fenced. Already
withdrawn identities cannot revive; exact committed retries use the stored receipt
and do not append another record. The new run fields necessarily change the wire
digest, so this is semantic retry after absent receipt, not an exact old-wire retry.

Journals are bounded64MiB; a record payload is at most8192 bytes and parsing bounds
allocation before reading it. Appends exceeding limits fail explicitly. Effects
are logical deletion, not secure erasure of SQLite/WAL/old snapshots.

Schema0015 exports use explicit format2 with five required entries and typed
coverage metadata. Format1's four-entry codec remains separate and unchanged.
See [encrypted exports](backup.md). The coverage boundary cannot reconstruct
previously unjournaled thread/mail deletions. Historical restore, latest off-host
checkpoints, media recovery and restore activation remain outside this work.

Source-version updates can invalidate derived content without a separate direct
destructive record. These journals alone therefore do not prove complete current
visibility when applied to an older database snapshot. Historical restore remains
unsupported; this change covers its listed direct intents and ordinary restart.
