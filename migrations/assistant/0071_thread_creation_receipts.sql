-- Owner-scoped request identity survives thread deletion; no thread foreign key.
-- NULL digest is a deletion tombstone, including journal replay before creation.
CREATE TABLE thread_creation_receipts (
 tenant_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 request_id TEXT NOT NULL,
 title_sha256 BLOB CHECK(title_sha256 IS NULL OR length(title_sha256)=32),
 PRIMARY KEY(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=71;
