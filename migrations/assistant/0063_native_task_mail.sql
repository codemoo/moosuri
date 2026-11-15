-- Actual supervised run/step/tool identities; legacy display jobs remain separate.
CREATE TABLE native_task_mail_bindings (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,device_id TEXT NOT NULL,binding_id TEXT NOT NULL,
 binding TEXT NOT NULL,PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE native_task_mail_reads (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 request_id TEXT NOT NULL,broker_call TEXT NOT NULL,invocation_nonce TEXT NOT NULL,
 payload TEXT NOT NULL,status TEXT NOT NULL,expires_ms INTEGER NOT NULL,
 acceptance_revision INTEGER NOT NULL,attempt INTEGER NOT NULL,request TEXT,
 PRIMARY KEY(tenant_id,run_id,step,call_id),UNIQUE(tenant_id,request_id),
 FOREIGN KEY(tenant_id,run_id,step,call_id) REFERENCES agent_tool_slots(tenant_id,run_id,step,call_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX native_task_mail_waits ON native_task_mail_reads(status,expires_ms);
CREATE TABLE native_task_mail_selections (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,request_id TEXT NOT NULL,payload TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,request_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER native_task_mail_delete BEFORE DELETE ON native_task_mail_reads BEGIN
 UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=OLD.tenant_id AND run_id=OLD.run_id AND step=OLD.step AND call_id=OLD.broker_call AND invocation_nonce=OLD.invocation_nonce;
END;

CREATE TABLE native_mail_context_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,origin_run TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,origin_run),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,origin_run) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;

PRAGMA user_version=63;
