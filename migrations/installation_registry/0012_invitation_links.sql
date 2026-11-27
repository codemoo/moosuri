-- One current one-use web link per reserved invitation. Only the SHA-256 of a
-- 32-byte random capability is stored, never the capability, a password, or a
-- session. Existing tables are untouched; rows become inert when any bound
-- issuer, invitee, route, or TTL fact stops matching.
CREATE TABLE IF NOT EXISTS installation_invitation_links (
    request_id TEXT PRIMARY KEY NOT NULL,
    digest BLOB NOT NULL UNIQUE CHECK(length(digest)=32),
    account_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    volume_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation>0),
    invitee_epoch INTEGER NOT NULL CHECK(invitee_epoch>=0),
    issuer_account_id TEXT NOT NULL,
    issuer_tenant_id TEXT NOT NULL,
    issuer_volume_id TEXT NOT NULL,
    issuer_generation INTEGER NOT NULL CHECK(issuer_generation>0),
    issuer_role_revision INTEGER NOT NULL CHECK(issuer_role_revision>0),
    expires_at INTEGER NOT NULL
) STRICT;
PRAGMA user_version=12;
