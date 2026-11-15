-- One exact save intent is dispatched at most once, including after lost ACKs.
-- No body, credentials or replacement send authority is stored here.
CREATE TABLE mail_provider_saves (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, action_id TEXT NOT NULL,
 operation_digest TEXT NOT NULL, action_revision INTEGER NOT NULL,
 connection_id TEXT NOT NULL, connection_epoch INTEGER NOT NULL, auth_epoch INTEGER NOT NULL,
 nonce BLOB NOT NULL, state TEXT NOT NULL CHECK(state IN('started','saved','outcome_unknown','quarantined')),
 receipt TEXT, created_at INTEGER NOT NULL, mime_sha256 TEXT NOT NULL,
 PRIMARY KEY(tenant_id,action_id,operation_digest),
 FOREIGN KEY(tenant_id,action_id) REFERENCES mail_actions(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=62;
