-- Durable research outcomes (shopping comparison / grant application). An outcome stores only the member's
-- criteria and *references* to sources already held in the thread (browser runs, Gmail thread reads).
-- Page text, mail metadata, offers, facts, drafts and receipts are re-derived on every read under live
-- authority, so withdrawal, auth-epoch change and generation change reach saved outcomes immediately.
-- Nothing here is chat/model input.
CREATE TABLE member_research_outcomes (
 id TEXT PRIMARY KEY NOT NULL,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0), thread_id TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('shopping','application')),
 title TEXT NOT NULL, message TEXT NOT NULL,
 criteria TEXT NOT NULL CHECK(json_valid(criteria)),
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 auth_epoch INTEGER NOT NULL, host_epoch INTEGER NOT NULL,
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 auto_sources INTEGER NOT NULL CHECK(auto_sources IN (0,1)),
 draft_requested INTEGER NOT NULL DEFAULT 0 CHECK(draft_requested IN (0,1)),
 question_id TEXT, pending_url TEXT,
 withdrawn INTEGER NOT NULL DEFAULT 0 CHECK(withdrawn IN (0,1)),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 admitted_order INTEGER NOT NULL UNIQUE,
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_research_outcome_thread
 ON member_research_outcomes(tenant_id,account_id,thread_id,admitted_order DESC);

-- A pinned reference. ref_id is a browser run id or a Gmail thread-read request id. host is the page host
-- observed when pinned (browser only); a later different host marks the source as changed.
CREATE TABLE member_research_outcome_sources (
 outcome_id TEXT NOT NULL REFERENCES member_research_outcomes(id) ON DELETE CASCADE,
 position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 11),
 kind TEXT NOT NULL CHECK(kind IN ('browser','mail')),
 ref_id TEXT NOT NULL, host TEXT, digest TEXT NOT NULL, pinned_at INTEGER NOT NULL,
 PRIMARY KEY(outcome_id,ref_id), UNIQUE(outcome_id,position)
) STRICT;

-- One applied transition per observed revision: an exact replay succeeds, any other use is stale.
CREATE TABLE member_research_outcome_actions (
 outcome_id TEXT NOT NULL REFERENCES member_research_outcomes(id) ON DELETE CASCADE,
 from_revision INTEGER NOT NULL, action TEXT NOT NULL, arg TEXT NOT NULL,
 PRIMARY KEY(outcome_id,from_revision)
) STRICT;

PRAGMA user_version=97;
