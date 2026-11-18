-- Capability proof is independent of text eligibility and account catalog metadata.
CREATE TABLE claude_code_tool_probes(
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, binding_id TEXT NOT NULL,
 revision INTEGER NOT NULL, model TEXT NOT NULL, effort TEXT NOT NULL,
 harness TEXT NOT NULL, verified_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,binding_id,revision,model,effort,harness),
 FOREIGN KEY(tenant_id,account_id,binding_id,revision,model)
  REFERENCES claude_code_probes(tenant_id,account_id,binding_id,revision,model) ON DELETE CASCADE
) STRICT;
-- Only opaque attempt identity and reservations; no model content or credentials.
CREATE TABLE claude_code_tool_probe_attempts(
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, binding_id TEXT NOT NULL,
 revision INTEGER NOT NULL, model TEXT NOT NULL, effort TEXT NOT NULL,
 harness TEXT NOT NULL, id TEXT NOT NULL, reserved_rounds INTEGER NOT NULL CHECK(reserved_rounds BETWEEN 0 AND 2),
 expires_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,account_id,binding_id,revision,model)
  REFERENCES claude_code_probes(tenant_id,account_id,binding_id,revision,model) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=78;
