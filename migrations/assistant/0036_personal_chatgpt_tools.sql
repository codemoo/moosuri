-- Capability proof is scoped by the existing owner/binding/revision/model/effort key.
ALTER TABLE chatgpt_model_probes ADD COLUMN function_tools INTEGER NOT NULL DEFAULT 0 CHECK(function_tools IN (0,1));
PRAGMA user_version=36;
