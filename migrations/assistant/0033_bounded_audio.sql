-- Bounded temporary dictation, separate from model capabilities and media storage.
CREATE TABLE audio_authority(singleton INTEGER PRIMARY KEY CHECK(singleton=1),generation INTEGER NOT NULL CHECK(generation>=1)) STRICT;
INSERT INTO audio_authority VALUES(1,1);
CREATE TRIGGER audio_authority_no_delete BEFORE DELETE ON audio_authority BEGIN SELECT RAISE(ABORT,'audio authority is permanent'); END;
CREATE TRIGGER audio_authority_monotonic BEFORE UPDATE ON audio_authority WHEN NEW.singleton!=OLD.singleton OR NEW.generation<=OLD.generation BEGIN SELECT RAISE(ABORT,'audio authority must advance'); END;
CREATE TABLE audio_selection(singleton INTEGER PRIMARY KEY CHECK(singleton=1),provider_id TEXT NOT NULL,provider_revision INTEGER NOT NULL,model TEXT NOT NULL) STRICT;
CREATE TABLE audio_jobs(
 id TEXT PRIMARY KEY NOT NULL,tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,session_digest BLOB NOT NULL,auth_epoch INTEGER NOT NULL,
 input_digest BLOB NOT NULL,provider_id TEXT NOT NULL,provider_revision INTEGER NOT NULL,model TEXT NOT NULL,
 authority_generation INTEGER NOT NULL,nonce TEXT NOT NULL,state TEXT NOT NULL,transcript TEXT,error TEXT,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,
 duration_ms INTEGER NOT NULL,dispatched INTEGER NOT NULL DEFAULT 0 CHECK(dispatched IN(0,1)),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 CHECK(state IN('processing','ready','cancelled','expired','outcome_unknown')),
 CHECK(transcript IS NULL OR state='ready')
) STRICT;
CREATE INDEX audio_jobs_owner ON audio_jobs(account_id,created_at);
PRAGMA user_version=33;
