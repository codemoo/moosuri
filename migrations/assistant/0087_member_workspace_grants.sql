-- Preserve legacy Gmail ciphertext and isolate each Workspace consent.
ALTER TABLE member_google_grants RENAME TO member_google_grants_v86;
CREATE TABLE member_google_grants (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0),app_revision TEXT NOT NULL,
 app_version INTEGER NOT NULL,connection_id TEXT,epoch INTEGER NOT NULL CHECK(epoch>=0),
 credential_version INTEGER NOT NULL CHECK(credential_version>=0),auth_epoch INTEGER NOT NULL,
 consent_digest BLOB NOT NULL CHECK(length(consent_digest)=32),mailbox TEXT,
 status TEXT NOT NULL CHECK(status IN('absent','active','disconnected','needs_reauth')),
 cipher BLOB,expires_at INTEGER,refresh_nonce BLOB,refresh_request_id TEXT,
 refresh_phase TEXT NOT NULL CHECK(refresh_phase IN('idle','dispatched','unknown')),
 service TEXT NOT NULL DEFAULT 'gmail' CHECK(service IN ('gmail','drive','docs','slides','sheets')),
 PRIMARY KEY(tenant_id,account_id,service),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id,app_revision) REFERENCES member_google_apps(tenant_id,account_id,app_revision),
 CHECK((status='active' AND cipher IS NOT NULL AND mailbox IS NOT NULL AND connection_id IS NOT NULL AND expires_at IS NOT NULL) OR (status!='active' AND cipher IS NULL AND expires_at IS NULL)),
 CHECK((refresh_phase='dispatched' AND refresh_nonce IS NOT NULL AND length(refresh_nonce)=32 AND refresh_request_id IS NOT NULL) OR (refresh_phase!='dispatched' AND refresh_nonce IS NULL AND refresh_request_id IS NULL))
) STRICT;
INSERT INTO member_google_grants SELECT *, 'gmail' FROM member_google_grants_v86;
DROP TABLE member_google_grants_v86;
ALTER TABLE member_google_oauth ADD COLUMN service TEXT NOT NULL DEFAULT 'gmail' CHECK(service IN ('gmail','drive','docs','slides','sheets'));
PRAGMA user_version=87;
