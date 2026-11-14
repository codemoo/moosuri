-- Public discovery metadata only. Installation/credentials remain per-user.
CREATE TABLE community_catalog_cache (
 source_key TEXT PRIMARY KEY, source TEXT NOT NULL,
 index_json TEXT, index_digest TEXT, etag TEXT,
 verified_at INTEGER, checked_at INTEGER NOT NULL DEFAULT 0,
 error TEXT, generation INTEGER NOT NULL DEFAULT 0,
 lease_nonce TEXT, lease_until INTEGER
) STRICT;
PRAGMA user_version=35;
