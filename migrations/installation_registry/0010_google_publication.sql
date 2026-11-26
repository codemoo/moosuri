-- Per-command intent/confirmation for the two PUBLICATION mutations that can only happen
-- after the OAuth client is saved (checkpoint step 'registered'): setting the installation's own
-- public information URLs on the Console Branding page ('domains') and moving the External
-- audience from Testing to In production ('publish').
-- Deliberately a SEPARATE table from installation_google_console_commands and deliberately
-- NOT backfilled: rows of migration 8/9 (a confirmed branding/audience/scopes, a Registered
-- checkpoint) say nothing about publication and can never be read as proof of it. A setup that
-- is already 'registered' starts here with no rows, i.e. both commands untried. The bootstrap
-- step/intent bit is not used (Registered carries intent=0), so a lost-client intent at step
-- 'client' is neither set nor consumed by these commands.
-- NOT evidence that any provider write happened or that Google verified the app; no secrets,
-- no free text. No foreign key: rows are keyed by the permanent setup id and never deleted.
CREATE TABLE installation_google_publication_commands (
    setup_id TEXT NOT NULL CHECK(length(setup_id)>0),
    command TEXT NOT NULL CHECK(command IN ('domains','publish')),
    state TEXT NOT NULL CHECK(state IN ('intent','confirmed')),
    attempts INTEGER NOT NULL DEFAULT 1 CHECK(attempts BETWEEN 1 AND 3),
    PRIMARY KEY(setup_id, command)
) STRICT;
-- A command begins once, as intent, only for the registered setup, and publish only after the
-- domains readback was confirmed. Even REPLACE cannot reset an entry.
CREATE TRIGGER installation_google_publication_commands_insert_intent
BEFORE INSERT ON installation_google_publication_commands
WHEN NEW.state<>'intent' OR NEW.attempts<>1
    OR NOT EXISTS(SELECT 1 FROM installation_google_bootstrap
        WHERE setup_id=NEW.setup_id AND step='registered' AND intent=0)
    OR EXISTS(SELECT 1 FROM installation_google_publication_commands
        WHERE setup_id=NEW.setup_id AND command=NEW.command)
    OR (NEW.command='publish' AND NOT EXISTS(SELECT 1 FROM installation_google_publication_commands
        WHERE setup_id=NEW.setup_id AND command='domains' AND state='confirmed'))
BEGIN SELECT RAISE(ABORT,'google publication command must begin once as intent'); END;
CREATE TRIGGER installation_google_publication_commands_monotone
BEFORE UPDATE ON installation_google_publication_commands
WHEN NOT (OLD.state='intent' AND NEW.setup_id=OLD.setup_id AND NEW.command=OLD.command
    AND ((NEW.state='confirmed' AND NEW.attempts=OLD.attempts)
        OR (NEW.state='intent' AND NEW.attempts=OLD.attempts+1)))
BEGIN SELECT RAISE(ABORT,'google publication command state is monotone'); END;
CREATE TRIGGER installation_google_publication_commands_no_delete
BEFORE DELETE ON installation_google_publication_commands
BEGIN SELECT RAISE(ABORT,'google publication command journal is permanent'); END;
PRAGMA user_version=10;
