-- Matching offline restoration of supported sealed-current checkpoints.
CREATE TABLE installation_restores (
 restore_id TEXT PRIMARY KEY NOT NULL CHECK(length(restore_id)=36),
 new_installation_id TEXT NOT NULL UNIQUE CHECK(length(new_installation_id)=36),
 source_installation_id TEXT NOT NULL CHECK(length(source_installation_id)=36),
 journal_installation_id TEXT NOT NULL CHECK(length(journal_installation_id)=36),
 seal_id TEXT NOT NULL CHECK(length(seal_id)=36),
 checkpoint_id TEXT NOT NULL CHECK(length(checkpoint_id)=36),
 ciphertext_sha256 TEXT NOT NULL CHECK(length(ciphertext_sha256)=64),
 trusted_head TEXT NOT NULL CHECK(length(CAST(trusted_head AS BLOB))<=8192),
 restored_at INTEGER NOT NULL
) STRICT;
CREATE TRIGGER installation_restores_no_update BEFORE UPDATE ON installation_restores
BEGIN SELECT RAISE(ABORT,'restore provenance immutable'); END;
CREATE TRIGGER installation_restores_no_delete BEFORE DELETE ON installation_restores
BEGIN SELECT RAISE(ABORT,'restore provenance immutable'); END;
PRAGMA user_version=20;
