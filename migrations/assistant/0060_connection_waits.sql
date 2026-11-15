-- Typed admitted slots and finite waits. Private grant pins contain no tokens or vault metadata.
CREATE TABLE mission_connection_slots (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,slot_id TEXT NOT NULL,
 operation TEXT NOT NULL,resource TEXT NOT NULL,schema_version INTEGER NOT NULL,
 account_scope TEXT NOT NULL,selection TEXT,expires_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,slot_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE mission_connection_waits (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,run_id TEXT NOT NULL,slot_id TEXT NOT NULL,
 acceptance_revision INTEGER NOT NULL,mission_revision INTEGER NOT NULL,step INTEGER NOT NULL,
 request_id TEXT NOT NULL,cause TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN('waiting','ready','resumed','expired','cancelled')),
 expires_ms INTEGER NOT NULL,created_order INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,mission_id,acceptance_revision),UNIQUE(tenant_id,request_id),
 FOREIGN KEY(tenant_id,run_id,slot_id) REFERENCES mission_connection_slots(tenant_id,run_id,slot_id) ON DELETE CASCADE
) STRICT;
CREATE TABLE mission_connection_selections (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,request_id TEXT NOT NULL,payload TEXT NOT NULL,
 PRIMARY KEY(tenant_id,mission_id,request_id),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE connection_aliases (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,binding_id TEXT NOT NULL,alias TEXT NOT NULL,
 PRIMARY KEY(tenant_id,account_id,binding_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX mission_wait_ready ON mission_connection_waits(status,created_order);
PRAGMA user_version=60;
