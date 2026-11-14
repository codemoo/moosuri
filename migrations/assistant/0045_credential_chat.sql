ALTER TABLE credential_broker_jobs ADD COLUMN step INTEGER NOT NULL DEFAULT 1 CHECK(step>0);
-- One terminal metadata read per chat run. All identities are original scoped IDs.
CREATE TABLE credential_chat_waits (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,attempt INTEGER NOT NULL,
 step INTEGER NOT NULL,call_id TEXT NOT NULL,broker_call TEXT NOT NULL,
 invocation_nonce TEXT NOT NULL,request_id TEXT NOT NULL,repository TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN('waiting','settled','cancelled')),
 expires_at INTEGER NOT NULL,PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,request_id) REFERENCES credential_broker_jobs(tenant_id,request_id)
) STRICT;
CREATE TRIGGER credential_chat_delete BEFORE DELETE ON credential_chat_waits BEGIN
 UPDATE credential_broker_jobs SET expires_at=min(expires_at,unixepoch()) WHERE tenant_id=OLD.tenant_id AND run_id=OLD.run_id;
 UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=OLD.tenant_id AND run_id=OLD.run_id;
END;
PRAGMA user_version=45;
