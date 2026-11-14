-- Staged imports never replace a working current session before verified commit.
CREATE TABLE chatgpt_handoffs (
 id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, account_id TEXT NOT NULL,
 session_digest BLOB NOT NULL CHECK(length(session_digest)=32), auth_epoch INTEGER NOT NULL,
 runtime_id TEXT NOT NULL, target_host TEXT NOT NULL, binding_id TEXT NOT NULL,
 expected_revision INTEGER NOT NULL, staged_revision INTEGER NOT NULL,
 transfer_key BLOB, nonce_commitment BLOB CHECK(nonce_commitment IS NULL OR length(nonce_commitment)=32),
 source_host TEXT, issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 phase TEXT NOT NULL CHECK(phase IN('prepared','committed','validating','imported','cancelled','expired')),
 import_digest BLOB CHECK(import_digest IS NULL OR length(import_digest)=32), lease_nonce TEXT, lease_until INTEGER,
 receipt TEXT, receipt_mac BLOB,
 FOREIGN KEY(tenant_id,account_id,binding_id,staged_revision) REFERENCES chatgpt_binding_revisions(tenant_id,account_id,binding_id,revision)
) STRICT;
CREATE UNIQUE INDEX chatgpt_handoff_active ON chatgpt_handoffs(tenant_id,account_id,binding_id,expected_revision) WHERE phase IN('prepared','committed','validating');
CREATE TRIGGER chatgpt_handoff_session_logout AFTER DELETE ON sessions BEGIN
 UPDATE chatgpt_handoffs SET phase='cancelled',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL WHERE session_digest=OLD.digest AND phase IN('prepared','committed','validating');
END;
CREATE TRIGGER chatgpt_handoff_account_epoch AFTER UPDATE OF auth_epoch,status ON accounts WHEN NEW.auth_epoch!=OLD.auth_epoch OR NEW.status!='active' BEGIN
 UPDATE chatgpt_handoffs SET phase='cancelled',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL WHERE tenant_id=NEW.tenant_id AND account_id=NEW.id AND phase IN('prepared','committed','validating');
END;
CREATE TRIGGER chatgpt_handoff_binding_change AFTER UPDATE OF current_revision,status ON chatgpt_bindings WHEN NEW.current_revision!=OLD.current_revision OR NEW.status IN('revoking','disconnected','connecting') BEGIN
 UPDATE chatgpt_handoffs SET phase='cancelled',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL WHERE tenant_id=NEW.tenant_id AND account_id=NEW.account_id AND binding_id=NEW.id AND phase IN('prepared','committed','validating');
END;
-- Purpose and immutable absolute lifetime are assigned at password-authenticated
-- issuance, before the cookie leaves the server. Browser sessions remain normal.
ALTER TABLE sessions ADD COLUMN purpose TEXT NOT NULL DEFAULT 'normal' CHECK(purpose IN('normal','chatgpt_helper'));
ALTER TABLE sessions ADD COLUMN absolute_deadline INTEGER;
CREATE TRIGGER session_purpose_valid_insert BEFORE INSERT ON sessions WHEN (NEW.purpose='normal' AND NEW.absolute_deadline IS NOT NULL) OR (NEW.purpose='chatgpt_helper' AND (NEW.absolute_deadline IS NULL OR NEW.expires_at>NEW.absolute_deadline)) BEGIN
 SELECT RAISE(ABORT,'invalid session purpose deadline');
END;
CREATE TRIGGER session_purpose_immutable BEFORE UPDATE OF purpose,absolute_deadline ON sessions WHEN NEW.purpose!=OLD.purpose OR NEW.absolute_deadline IS NOT OLD.absolute_deadline BEGIN
 SELECT RAISE(ABORT,'session purpose deadline is immutable');
END;
PRAGMA user_version=34;
