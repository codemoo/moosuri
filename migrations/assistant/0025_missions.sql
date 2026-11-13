-- Durable missions follow vault authority24.
ALTER TABLE threads ADD COLUMN mission_internal INTEGER NOT NULL DEFAULT 0 CHECK(mission_internal IN(0,1));
CREATE TABLE missions (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,account_id TEXT NOT NULL,run_id TEXT NOT NULL,thread_id TEXT NOT NULL,
 request_id TEXT NOT NULL,payload TEXT NOT NULL,payload_digest BLOB NOT NULL,mode TEXT NOT NULL CHECK(mode IN('current_session','durable')),
 auth_epoch INTEGER NOT NULL,revision INTEGER NOT NULL DEFAULT 1,state TEXT NOT NULL DEFAULT 'queued',grant_enabled INTEGER NOT NULL DEFAULT 1,
 steering TEXT NOT NULL DEFAULT '[]',plan TEXT NOT NULL DEFAULT '[]',candidate TEXT,report TEXT NOT NULL DEFAULT '[]',repairs INTEGER NOT NULL DEFAULT 0 CHECK(repairs BETWEEN 0 AND 2),
 uncertain INTEGER NOT NULL DEFAULT 0,expires_ms INTEGER NOT NULL,round_text TEXT,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,request_id),UNIQUE(tenant_id,run_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE mission_revisions (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,revision INTEGER NOT NULL,request_id TEXT NOT NULL,payload TEXT NOT NULL,
 PRIMARY KEY(tenant_id,mission_id,revision),UNIQUE(tenant_id,mission_id,request_id)
) STRICT;
CREATE TABLE mission_controls (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,request_id TEXT NOT NULL,payload TEXT NOT NULL,response TEXT NOT NULL,
 PRIMARY KEY(tenant_id,mission_id,request_id)
) STRICT;
CREATE INDEX mission_run ON missions(tenant_id,run_id);
PRAGMA user_version=25;
