-- Immutable equality evidence is committed before raw MIME can be purged.
CREATE TABLE mail_send_evidence (
 tenant_id TEXT NOT NULL,action_id TEXT NOT NULL,account_id TEXT NOT NULL,
 revision INTEGER NOT NULL,digest TEXT NOT NULL,connection_id TEXT NOT NULL,connection_epoch INTEGER NOT NULL,
 auth_epoch INTEGER NOT NULL,dispatch_nonce BLOB NOT NULL,message_id TEXT NOT NULL,reply_thread TEXT,
 committed_at INTEGER NOT NULL,oracle_version INTEGER NOT NULL CHECK(oracle_version IN(0,1)),fingerprint TEXT,
 withdrawn INTEGER NOT NULL DEFAULT 0 CHECK(withdrawn IN(0,1)),
 PRIMARY KEY(tenant_id,action_id),FOREIGN KEY(tenant_id,action_id) REFERENCES mail_actions(tenant_id,id) ON DELETE CASCADE,
 CHECK((oracle_version=0 AND fingerprint IS NULL) OR (oracle_version=1 AND length(fingerprint)=64))
) STRICT;
CREATE TRIGGER mail_send_evidence_immutable BEFORE UPDATE ON mail_send_evidence
WHEN NEW.tenant_id!=OLD.tenant_id OR NEW.action_id!=OLD.action_id OR NEW.account_id!=OLD.account_id OR NEW.revision!=OLD.revision OR NEW.digest!=OLD.digest OR NEW.connection_id!=OLD.connection_id OR NEW.connection_epoch!=OLD.connection_epoch OR NEW.auth_epoch!=OLD.auth_epoch OR NEW.dispatch_nonce!=OLD.dispatch_nonce OR NEW.message_id!=OLD.message_id OR NEW.reply_thread IS NOT OLD.reply_thread OR NEW.committed_at!=OLD.committed_at OR NEW.oracle_version!=OLD.oracle_version OR NEW.fingerprint IS NOT OLD.fingerprint OR NEW.withdrawn<OLD.withdrawn
BEGIN SELECT RAISE(ABORT,'immutable mail evidence'); END;
CREATE TABLE mail_reconciliations (
 tenant_id TEXT NOT NULL,action_id TEXT NOT NULL,
 automatic_attempts INTEGER NOT NULL DEFAULT 0 CHECK(automatic_attempts BETWEEN 0 AND 4),
 next_due INTEGER,manual_after INTEGER NOT NULL DEFAULT 0,
 lease_nonce BLOB,lease_until INTEGER,quarantined INTEGER NOT NULL DEFAULT 0 CHECK(quarantined IN(0,1)),
 status TEXT NOT NULL DEFAULT 'not_checked',last_result TEXT NOT NULL DEFAULT 'not_checked',last_checked_at INTEGER,candidate_count INTEGER NOT NULL DEFAULT 0 CHECK(candidate_count BETWEEN 0 AND 5),
 provider_message TEXT,provider_thread TEXT,conflict_message TEXT,conflict_thread TEXT,
 PRIMARY KEY(tenant_id,action_id),FOREIGN KEY(tenant_id,action_id) REFERENCES mail_send_evidence(tenant_id,action_id) ON DELETE CASCADE
) STRICT;
-- Historical rows explicitly lack an equality oracle; never infer it from a candidate.
INSERT INTO mail_send_evidence(tenant_id,action_id,account_id,revision,digest,connection_id,connection_epoch,auth_epoch,dispatch_nonce,message_id,reply_thread,committed_at,oracle_version)
 SELECT tenant_id,id,account_id,revision,digest,connection_id,connection_epoch,auth_epoch,dispatch_nonce,message_id,reply_thread,0,0 FROM mail_actions WHERE state IN('dispatch_committed','outcome_unknown') AND dispatch_nonce IS NOT NULL;
INSERT INTO mail_reconciliations(tenant_id,action_id,next_due,status) SELECT tenant_id,action_id,0,'legacy_evidence_unavailable' FROM mail_send_evidence;
CREATE INDEX mail_reconciliation_due ON mail_reconciliations(quarantined,next_due);
PRAGMA user_version=31;
