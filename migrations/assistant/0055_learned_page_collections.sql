-- Individual public reads share the original parent slot and run/step fences.
CREATE TABLE learned_page_dispatches (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 3),nonce BLOB NOT NULL,
 state TEXT NOT NULL CHECK(state IN('started','completed')),
 PRIMARY KEY(tenant_id,run_id,step,call_id,ordinal),
 FOREIGN KEY(tenant_id,run_id,step,call_id) REFERENCES agent_tool_slots(tenant_id,run_id,step,call_id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=55;
