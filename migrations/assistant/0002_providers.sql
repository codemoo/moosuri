-- Shared connection revisions are immutable; personal selections remain tenant-scoped.
CREATE TABLE provider_key_meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    key_check BLOB NOT NULL
) STRICT;
CREATE TABLE providers (
    id TEXT PRIMARY KEY NOT NULL,
    current_revision INTEGER NOT NULL CHECK(current_revision>0),
    FOREIGN KEY(id,current_revision) REFERENCES provider_revisions(provider_id,revision)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE provider_revisions (
    provider_id TEXT NOT NULL REFERENCES providers(id) DEFERRABLE INITIALLY DEFERRED,
    revision INTEGER NOT NULL CHECK(revision>0),
    name TEXT NOT NULL,
    config TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    credential BLOB,
    PRIMARY KEY(provider_id,revision)
) STRICT;
-- Retire old ciphertext after moving the current pointer; public revision metadata stays immutable.
CREATE TRIGGER provider_revisions_no_update BEFORE UPDATE ON provider_revisions
WHEN NOT (
    NEW.provider_id IS OLD.provider_id AND NEW.revision IS OLD.revision
    AND NEW.name IS OLD.name AND NEW.config IS OLD.config AND NEW.enabled IS OLD.enabled
    AND OLD.credential IS NOT NULL AND NEW.credential IS NULL
    AND OLD.revision != (SELECT current_revision FROM providers WHERE id=OLD.provider_id)
)
BEGIN SELECT RAISE(ABORT,'immutable revision'); END;
CREATE TRIGGER provider_revisions_no_delete BEFORE DELETE ON provider_revisions
BEGIN SELECT RAISE(ABORT,'immutable revision'); END;
CREATE TABLE provider_model_probes (
    provider_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    verified_at INTEGER NOT NULL,
    PRIMARY KEY(provider_id,revision,model,effort),
    FOREIGN KEY(provider_id,revision) REFERENCES provider_revisions(provider_id,revision)
) STRICT;
CREATE TABLE installation_model_selection (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    provider_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    FOREIGN KEY(provider_id,revision,model,effort)
        REFERENCES provider_model_probes(provider_id,revision,model,effort)
) STRICT;
CREATE TABLE personal_model_selections (
    tenant_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    PRIMARY KEY(tenant_id,account_id),
    FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
    FOREIGN KEY(provider_id,revision,model,effort)
        REFERENCES provider_model_probes(provider_id,revision,model,effort)
) STRICT;
PRAGMA user_version=2;
