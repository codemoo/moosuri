-- No backfill: old free-text plans receive no artifact authority.
CREATE TABLE mission_report_contracts (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, acceptance_revision INTEGER NOT NULL,
 account_id TEXT NOT NULL, schema_version INTEGER NOT NULL CHECK(schema_version=1),
 profile_sha256 TEXT NOT NULL, mutation_id TEXT NOT NULL, contract TEXT NOT NULL,
 artifact_id TEXT, artifact_revision INTEGER,
 PRIMARY KEY(tenant_id,run_id,acceptance_revision), UNIQUE(tenant_id,mutation_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX mission_report_artifact ON mission_report_contracts(tenant_id,artifact_id,artifact_revision);
PRAGMA user_version=59;
