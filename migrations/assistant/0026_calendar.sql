-- Account-owned Calendar credentials and durable operation receipts.
CREATE TABLE calendar_connections (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,primary_id TEXT NOT NULL,title TEXT NOT NULL,time_zone TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL,epoch INTEGER NOT NULL DEFAULT 1,credential_revision INTEGER NOT NULL DEFAULT 1,
 status TEXT NOT NULL CHECK(status IN('active','needs_reconnect','disconnected')),credential BLOB,expires_at INTEGER NOT NULL,scopes TEXT NOT NULL,client_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,primary_id),FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE calendar_oauth_sessions (
 state_hash BLOB PRIMARY KEY NOT NULL,tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,session_digest BLOB NOT NULL,auth_epoch INTEGER NOT NULL,
 expires_at INTEGER NOT NULL,verifier BLOB,code BLOB,result_hash BLOB,claimed INTEGER NOT NULL DEFAULT 0,app_pin TEXT NOT NULL,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE calendar_actions (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,connection_id TEXT NOT NULL,connection_epoch INTEGER NOT NULL,credential_revision INTEGER NOT NULL,
 auth_epoch INTEGER NOT NULL,session_digest BLOB NOT NULL,app_pin TEXT NOT NULL,payload TEXT,digest TEXT NOT NULL,kind TEXT NOT NULL,event_id TEXT NOT NULL,before_etag TEXT,
 state TEXT NOT NULL,nonce BLOB NOT NULL,provider_etag TEXT,event TEXT,receipt TEXT,error TEXT,version INTEGER NOT NULL DEFAULT 1,created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE INDEX calendar_actions_owner ON calendar_actions(tenant_id,account_id,created_at);
PRAGMA user_version=26;
