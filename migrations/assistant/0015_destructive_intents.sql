-- Local opaque successor journal. The installation binding/frozen legacy prefix
-- and initial header head are inserted by the Rust migration after file fsync.
CREATE TABLE destructive_journal_meta (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 installation_id TEXT NOT NULL CHECK(length(installation_id)=36),
 legacy_sequence INTEGER NOT NULL CHECK(legacy_sequence>=0),
 legacy_head BLOB NOT NULL CHECK(length(legacy_head)=32),
 sequence INTEGER NOT NULL CHECK(sequence>=0),
 head BLOB NOT NULL CHECK(length(head)=32)
) STRICT;
-- Standalone worker starts at13; main applies automatic-memory0014 then uses15.
PRAGMA user_version=15;
