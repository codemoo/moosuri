-- Exact private receipt provenance is Store-derived, never caller-supplied authority.
CREATE TABLE artifact_external_profiles (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,artifact_id TEXT,artifact_revision INTEGER,PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE artifact_external_context (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,origin_run TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,origin_run),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX artifact_external_context_origin ON artifact_external_context(tenant_id,origin_run);
CREATE TABLE artifact_external_evidence (
 tenant_id TEXT NOT NULL,artifact_id TEXT NOT NULL,artifact_revision INTEGER NOT NULL,
 origin_run TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN('web','mail','calendar')),request TEXT NOT NULL,
 request_sha256 TEXT NOT NULL,result_sha256 TEXT NOT NULL,
 PRIMARY KEY(tenant_id,artifact_id,artifact_revision,origin_run,step,call_id),
 FOREIGN KEY(tenant_id,artifact_id,artifact_revision) REFERENCES personal_artifact_versions(tenant_id,artifact_id,revision)
) STRICT;
CREATE INDEX artifact_external_evidence_origin ON artifact_external_evidence(tenant_id,origin_run);
PRAGMA user_version=54;
