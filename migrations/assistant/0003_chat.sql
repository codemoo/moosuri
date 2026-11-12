ALTER TABLE threads ADD COLUMN chat_turn_seq INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN chat_event_seq INTEGER NOT NULL DEFAULT 0;
CREATE TABLE chat_runs (
    tenant_id TEXT NOT NULL,
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    payload_digest BLOB NOT NULL CHECK(length(payload_digest)=32),
    sequence INTEGER NOT NULL,
    user_text TEXT NOT NULL,
    model TEXT NOT NULL,
    session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
    auth_epoch INTEGER NOT NULL,
    attempt INTEGER NOT NULL CHECK(attempt BETWEEN 1 AND 32),
    state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed','cancelled','incomplete','refused','outcome_unknown')),
    version INTEGER NOT NULL,
    error TEXT,
    created_at INTEGER NOT NULL,
    admitted_order INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,id),
    UNIQUE(tenant_id,request_id),
    UNIQUE(tenant_id,thread_id,sequence),
    FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
    FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE UNIQUE INDEX chat_one_active_thread ON chat_runs(tenant_id,thread_id) WHERE state IN ('queued','running');
CREATE INDEX chat_queue ON chat_runs(state,admitted_order);
CREATE TABLE chat_output (
    tenant_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    item TEXT NOT NULL,
    id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    text TEXT NOT NULL,
    phase TEXT,
    refusal INTEGER NOT NULL CHECK(refusal IN (0,1)),
    PRIMARY KEY(tenant_id,run_id,item),
    UNIQUE(tenant_id,run_id,id),
    FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE chat_events (
    tenant_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    change TEXT NOT NULL,
    PRIMARY KEY(tenant_id,thread_id,sequence),
    FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE
) STRICT;
-- Deliberately no FK to prompts/threads: minimal accounting survives content deletion.
CREATE TABLE model_call_receipts (
    tenant_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    nonce BLOB NOT NULL CHECK(length(nonce)=32),
    usage TEXT NOT NULL,
    dispatched_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id,run_id,attempt),
    FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE chat_admission_counter (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    value INTEGER NOT NULL
) STRICT;
INSERT INTO chat_admission_counter VALUES (1,0);
PRAGMA user_version=3;
