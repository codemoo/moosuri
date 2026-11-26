-- Durable intent checkpoint for the one installation-wide Google OAuth app.
-- This is NOT evidence that any provider operation happened. No passwords,
-- client secrets, browser sessions, billing identifiers or member grants belong here.
CREATE TABLE installation_google_bootstrap (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    setup_id TEXT NOT NULL,
    google_email TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    step TEXT NOT NULL CHECK(step IN ('project','apis','branding','audience','client','registered')),
    -- 1: the provider write for `step` may have reached Google; reconcile before repeating.
    intent INTEGER NOT NULL DEFAULT 0 CHECK(intent IN (0,1)),
    lease_id TEXT NOT NULL,
    lease_account TEXT NOT NULL,
    lease_generation INTEGER NOT NULL,
    lease_revision INTEGER NOT NULL,
    lease_epoch INTEGER NOT NULL,
    lease_until INTEGER NOT NULL,
    CHECK(step!='registered' OR intent=0)
) STRICT;
-- The stable resource identity can never be rewritten or discarded: a "reset"
-- would orphan or duplicate provider resources.
CREATE TRIGGER installation_google_bootstrap_identity_fixed
BEFORE UPDATE OF singleton,setup_id ON installation_google_bootstrap
BEGIN SELECT RAISE(ABORT,'google bootstrap identity is fixed'); END;
-- The account and callback may be corrected only while no provider write has been
-- attempted (step project, intent 0 before and after). The lease-ended check is in Rust.
CREATE TRIGGER installation_google_bootstrap_account_fixed
BEFORE UPDATE OF google_email,redirect_uri ON installation_google_bootstrap
WHEN (NEW.google_email IS NOT OLD.google_email OR NEW.redirect_uri IS NOT OLD.redirect_uri)
    AND NOT (OLD.step='project' AND OLD.intent=0 AND NEW.step='project' AND NEW.intent=0)
BEGIN SELECT RAISE(ABORT,'google bootstrap identity is fixed'); END;
CREATE TRIGGER installation_google_bootstrap_no_delete
BEFORE DELETE ON installation_google_bootstrap
BEGIN SELECT RAISE(ABORT,'google bootstrap checkpoint is permanent'); END;
-- Project -> Apis -> Branding -> Audience -> Client -> Registered, one confirmed step at a time.
CREATE TRIGGER installation_google_bootstrap_step_order
BEFORE UPDATE OF step ON installation_google_bootstrap
WHEN NEW.step IS NOT OLD.step AND NOT (
    OLD.intent=1 AND NEW.intent=0 AND (
        (OLD.step='project' AND NEW.step='apis') OR
        (OLD.step='apis' AND NEW.step='branding') OR
        (OLD.step='branding' AND NEW.step='audience') OR
        (OLD.step='audience' AND NEW.step='client') OR
        (OLD.step='client' AND NEW.step='registered')))
BEGIN SELECT RAISE(ABORT,'google bootstrap steps cannot be skipped or reversed'); END;
-- A worker id must never become valid again after it has been used.
CREATE TABLE installation_google_bootstrap_runs (
    run_id TEXT PRIMARY KEY
) STRICT;
CREATE TRIGGER installation_google_bootstrap_runs_append_only_update
BEFORE UPDATE ON installation_google_bootstrap_runs
BEGIN SELECT RAISE(ABORT,'used google bootstrap runs are permanent'); END;
CREATE TRIGGER installation_google_bootstrap_runs_append_only_delete
BEFORE DELETE ON installation_google_bootstrap_runs
BEGIN SELECT RAISE(ABORT,'used google bootstrap runs are permanent'); END;
PRAGMA user_version=8;
