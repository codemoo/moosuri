-- Member Calendar (one primary timed event create/move). Schema 96.
-- 1. Admit the 'calendar' service in the existing per-member Google grant/OAuth
--    tables. Tokens stay in member_google_grants (same encrypted location, same
--    owner/volume/generation/auth-epoch/app pin). No new token table exists.
-- 2. Short-lived candidate handles: server-side event id + ETag, never model input.
-- 3. A one-row-per-request action ledger: dispatch is committed before provider IO.
-- Requires migrations 93-95 to be registered first (version registry is contiguous).
ALTER TABLE member_google_grants RENAME TO member_google_grants_v95;
CREATE TABLE member_google_grants (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0),app_revision TEXT NOT NULL,
 app_version INTEGER NOT NULL,connection_id TEXT,epoch INTEGER NOT NULL CHECK(epoch>=0),
 credential_version INTEGER NOT NULL CHECK(credential_version>=0),auth_epoch INTEGER NOT NULL,
 consent_digest BLOB NOT NULL CHECK(length(consent_digest)=32),mailbox TEXT,
 status TEXT NOT NULL CHECK(status IN('absent','active','disconnected','needs_reauth')),
 cipher BLOB,expires_at INTEGER,refresh_nonce BLOB,refresh_request_id TEXT,
 refresh_phase TEXT NOT NULL CHECK(refresh_phase IN('idle','dispatched','unknown')),
 service TEXT NOT NULL DEFAULT 'gmail' CHECK(service IN ('gmail','drive','docs','slides','sheets','calendar')),
 PRIMARY KEY(tenant_id,account_id,service),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id,app_revision) REFERENCES member_google_apps(tenant_id,account_id,app_revision),
 CHECK((status='active' AND cipher IS NOT NULL AND mailbox IS NOT NULL AND connection_id IS NOT NULL AND expires_at IS NOT NULL) OR (status!='active' AND cipher IS NULL AND expires_at IS NULL)),
 CHECK((refresh_phase='dispatched' AND refresh_nonce IS NOT NULL AND length(refresh_nonce)=32 AND refresh_request_id IS NOT NULL) OR (refresh_phase!='dispatched' AND refresh_nonce IS NULL AND refresh_request_id IS NULL))
) STRICT;
INSERT INTO member_google_grants SELECT * FROM member_google_grants_v95;
DROP TABLE member_google_grants_v95;

ALTER TABLE member_google_oauth RENAME TO member_google_oauth_v95;
CREATE TABLE member_google_oauth (
 state_hash BLOB PRIMARY KEY NOT NULL REFERENCES mail_oauth_sessions(state_hash) ON DELETE CASCADE,
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0),app_revision TEXT NOT NULL,
 app_version INTEGER NOT NULL,epoch INTEGER NOT NULL,
 service TEXT NOT NULL DEFAULT 'gmail' CHECK(service IN ('gmail','drive','docs','slides','sheets','calendar')),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id,app_revision) REFERENCES member_google_apps(tenant_id,account_id,app_revision)
) STRICT;
INSERT INTO member_google_oauth SELECT * FROM member_google_oauth_v95;
DROP TABLE member_google_oauth_v95;

-- A candidate is an opaque handle to one provider event observed in one listing.
-- It carries ids/ETag only (no title/body) and is bound to the exact session,
-- auth epoch, volume generation, app pin and credential generation.
CREATE TABLE member_calendar_candidates (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,candidate_ref TEXT NOT NULL CHECK(length(candidate_ref)=32),
 volume_id TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL,session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
 app_revision TEXT NOT NULL,connection_id TEXT NOT NULL,connection_epoch INTEGER NOT NULL CHECK(connection_epoch>0),
 credential_version INTEGER NOT NULL,event_id TEXT NOT NULL,etag TEXT NOT NULL,
 created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,consumed_by TEXT,
 PRIMARY KEY(tenant_id,account_id,candidate_ref),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_calendar_candidates_expiry ON member_calendar_candidates(tenant_id,account_id,expires_at);

-- Provider id for create is derived from request_id (stable); the private
-- operation/digest marker is written to the event so a lost acknowledgement is
-- reconciled by read-back, never by a second write.
CREATE TABLE member_calendar_actions (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,request_id TEXT NOT NULL,
 volume_id TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL,session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
 app_revision TEXT NOT NULL,connection_id TEXT NOT NULL,connection_epoch INTEGER NOT NULL CHECK(connection_epoch>0),
 kind TEXT NOT NULL CHECK(kind IN('create','move')),
 event_id TEXT NOT NULL,before_etag TEXT,digest TEXT NOT NULL CHECK(length(digest)=64),
 state TEXT NOT NULL CHECK(state IN('dispatch_committed','applied','conflict','failed_definite','outcome_unknown','withdrawn')),
 confirmed_by TEXT CHECK(confirmed_by IN('provider_response','readback')),
 provider_etag TEXT,receipt TEXT,error TEXT,payload BLOB,event BLOB,
 version INTEGER NOT NULL CHECK(version>0),created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 CHECK((state='applied' AND confirmed_by IS NOT NULL) OR (state!='applied' AND confirmed_by IS NULL)),
 CHECK(state!='withdrawn' OR (payload IS NULL AND event IS NULL))
) STRICT;
CREATE INDEX member_calendar_actions_recent ON member_calendar_actions(tenant_id,account_id,created_at DESC);
PRAGMA user_version=96;
