CREATE TABLE personal_artifacts (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN('document','goal','followup')),
 revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 129),status TEXT NOT NULL CHECK(status IN('active','withdrawn','deleted')),
 updated_at INTEGER NOT NULL,PRIMARY KEY(tenant_id,id)
) STRICT;
CREATE TABLE personal_artifact_versions (
 tenant_id TEXT NOT NULL,artifact_id TEXT NOT NULL,revision INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN('active','withdrawn','deleted')),draft TEXT,created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,artifact_id,revision),
 FOREIGN KEY(tenant_id,artifact_id) REFERENCES personal_artifacts(tenant_id,id),
 CHECK((status='active' AND draft IS NOT NULL) OR (status!='active' AND draft IS NULL))
) STRICT;
CREATE TABLE personal_artifact_dependencies (
 tenant_id TEXT NOT NULL,artifact_id TEXT NOT NULL,artifact_revision INTEGER NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN('source','memory','artifact')),object_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(tenant_id,artifact_id,artifact_revision,kind,object_id,revision),
 FOREIGN KEY(tenant_id,artifact_id,artifact_revision) REFERENCES personal_artifact_versions(tenant_id,artifact_id,revision)
) STRICT;
CREATE INDEX personal_artifact_dependency_object ON personal_artifact_dependencies(tenant_id,kind,object_id,revision);
CREATE TABLE personal_artifact_mutations (
 tenant_id TEXT NOT NULL,request_id TEXT NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),
 artifact_id TEXT NOT NULL,revision INTEGER NOT NULL,status TEXT NOT NULL CHECK(status IN('active','deleted')),
 PRIMARY KEY(tenant_id,request_id)
) STRICT;
CREATE TABLE personal_artifact_command_plans (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,schema_version INTEGER NOT NULL CHECK(schema_version=1),
 action TEXT NOT NULL CHECK(action IN('create','edit')),kind TEXT NOT NULL CHECK(kind IN('document','goal','followup')),
 target_id TEXT,target_revision INTEGER,generation INTEGER NOT NULL,request_id TEXT NOT NULL,used INTEGER NOT NULL DEFAULT 0 CHECK(used IN(0,1)),
 PRIMARY KEY(tenant_id,run_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE personal_artifact_run_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,artifact_id TEXT NOT NULL,revision INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,artifact_id,revision),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=16;
-- Separate admission sidecar pins this additive vocabulary without upgrading old
-- RunToolProfile versions or granting historical runs newly deployed capabilities.
CREATE TABLE personal_artifact_run_profiles (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,schema_version INTEGER NOT NULL CHECK(schema_version=1),
 selected_id TEXT,selected_revision INTEGER,planned INTEGER NOT NULL DEFAULT 0 CHECK(planned IN(0,1)),
 PRIMARY KEY(tenant_id,run_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
