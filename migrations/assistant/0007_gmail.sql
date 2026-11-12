CREATE TABLE mail_connections (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,email TEXT NOT NULL,
 epoch INTEGER NOT NULL DEFAULT 1,credential_revision INTEGER NOT NULL DEFAULT 1,
 status TEXT NOT NULL CHECK(status IN('active','needs_reconnect','disconnected')),
 credential BLOB,expires_at INTEGER NOT NULL,scopes TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE mail_oauth_sessions (
 state_hash BLOB PRIMARY KEY NOT NULL,tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,
 session_digest BLOB NOT NULL,auth_epoch INTEGER NOT NULL,expires_at INTEGER NOT NULL,
 verifier BLOB,code BLOB,result_hash BLOB,claimed INTEGER NOT NULL DEFAULT 0,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE mail_actions (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL,
 version INTEGER NOT NULL DEFAULT 1,connection_id TEXT NOT NULL,connection_epoch INTEGER NOT NULL,auth_epoch INTEGER NOT NULL,
 content TEXT,raw BLOB,digest TEXT NOT NULL,message_id TEXT NOT NULL,reply_thread TEXT,
 origin_thread TEXT,origin_run TEXT,state TEXT NOT NULL CHECK(state IN('pending_approval','approved','dispatch_committed','sent','failed_definite','outcome_unknown','rejected','cancelled','expired')),
 expires_at INTEGER NOT NULL,dispatch_nonce BLOB,provider_message TEXT,provider_thread TEXT,
 PRIMARY KEY(tenant_id,id),FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE INDEX mail_action_queue ON mail_actions(state,expires_at);
CREATE TABLE external_tool_dispatches (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 nonce BLOB NOT NULL,state TEXT NOT NULL CHECK(state IN('started','completed')),
 PRIMARY KEY(tenant_id,run_id,step,call_id),
 FOREIGN KEY(tenant_id,run_id,step,call_id) REFERENCES agent_tool_slots(tenant_id,run_id,step,call_id) ON DELETE CASCADE
) STRICT;
CREATE TABLE mail_run_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,connection_id TEXT NOT NULL,epoch INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,connection_id,epoch),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=7;
