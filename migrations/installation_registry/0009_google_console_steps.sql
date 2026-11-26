-- Per-command Console intent/confirmation for the one installation Google setup. Same
-- workflow journal as installation_google_bootstrap (the Rust layer applies the same admin
-- pin, lease and SDK fence); this table only separates the provider mutations that the
-- historical single `intent` bit could not tell apart (audience test-user vs Data Access
-- scopes). NOT evidence that any provider write happened; no secrets, no free text.
-- No foreign key on purpose: rows are keyed by the permanent setup id and never deleted.
-- A setup at branding/audience with intent=1 and NO rows here is a migration-8 era
-- checkpoint and is read conservatively as uncertain by the Rust layer.
CREATE TABLE installation_google_console_commands (
    setup_id TEXT NOT NULL CHECK(length(setup_id)>0),
    command TEXT NOT NULL CHECK(command IN ('branding','audience','scopes')),
    state TEXT NOT NULL CHECK(state IN ('intent','confirmed')),
    PRIMARY KEY(setup_id, command)
) STRICT;
-- Even REPLACE must not reset an existing entry (SQLite does not run delete
-- triggers for REPLACE unless recursive_triggers is enabled).
CREATE TRIGGER installation_google_console_commands_insert_intent
BEFORE INSERT ON installation_google_console_commands
WHEN NEW.state<>'intent'
    OR NOT EXISTS(SELECT 1 FROM installation_google_bootstrap WHERE setup_id=NEW.setup_id)
    OR EXISTS(SELECT 1 FROM installation_google_console_commands
        WHERE setup_id=NEW.setup_id AND command=NEW.command)
BEGIN SELECT RAISE(ABORT,'google console command must begin once as intent'); END;
-- Monotone: a row may only move intent -> confirmed; it is never reset, rewritten or removed.
CREATE TRIGGER installation_google_console_commands_monotone
BEFORE UPDATE ON installation_google_console_commands
WHEN NOT (OLD.state='intent' AND NEW.state='confirmed'
    AND NEW.setup_id=OLD.setup_id AND NEW.command=OLD.command)
BEGIN SELECT RAISE(ABORT,'google console command state is monotone'); END;
CREATE TRIGGER installation_google_console_commands_no_delete
BEFORE DELETE ON installation_google_console_commands
BEGIN SELECT RAISE(ABORT,'google console command journal is permanent'); END;
PRAGMA user_version=9;
