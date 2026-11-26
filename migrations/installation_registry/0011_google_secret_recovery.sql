-- A separate bounded intent for replacing a lost, never-registered client secret.
-- Identity and attempt history survive worker/gateway restarts. This is not proof
-- that Google added a secret or that the private installation file was saved.
CREATE TABLE installation_google_secret_recovery (
    setup_id TEXT PRIMARY KEY NOT NULL,
    client_id TEXT NOT NULL CHECK(length(client_id) BETWEEN 1 AND 256),
    attempts INTEGER NOT NULL CHECK(attempts BETWEEN 1 AND 3)
) STRICT;
CREATE TRIGGER installation_google_secret_recovery_begin
BEFORE INSERT ON installation_google_secret_recovery
WHEN NEW.attempts<>1
 OR EXISTS(SELECT 1 FROM installation_google_secret_recovery WHERE setup_id=NEW.setup_id)
 OR NOT EXISTS(SELECT 1 FROM installation_google_bootstrap
     WHERE setup_id=NEW.setup_id AND step='client' AND intent=1)
BEGIN SELECT RAISE(ABORT,'secret recovery requires an unsaved client intent'); END;
CREATE TRIGGER installation_google_secret_recovery_progress
BEFORE UPDATE ON installation_google_secret_recovery
WHEN NEW.setup_id<>OLD.setup_id OR NEW.client_id<>OLD.client_id
 OR NEW.attempts<>OLD.attempts+1
 OR NOT EXISTS(SELECT 1 FROM installation_google_bootstrap
     WHERE setup_id=NEW.setup_id AND step='client' AND intent=1)
BEGIN SELECT RAISE(ABORT,'secret recovery identity and budget are permanent'); END;
CREATE TRIGGER installation_google_secret_recovery_no_delete
BEFORE DELETE ON installation_google_secret_recovery
BEGIN SELECT RAISE(ABORT,'secret recovery journal is permanent'); END;
PRAGMA user_version=11;
