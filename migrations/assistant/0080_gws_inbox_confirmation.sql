-- Explicit owner selection; no OAuth credentials or mailbox content is stored.
CREATE TABLE gws_inbox_selections (
    tenant_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    auth_epoch INTEGER NOT NULL,
    fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32),
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK(confirmed IN (0,1)),
    challenge_digest BLOB CHECK(challenge_digest IS NULL OR length(challenge_digest)=32),
    challenge_expires_at INTEGER,
    PRIMARY KEY(tenant_id, account_id),
    FOREIGN KEY(tenant_id, account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version = 80;
