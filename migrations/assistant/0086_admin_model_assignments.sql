CREATE TABLE admin_model_assignments (
    tenant_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    PRIMARY KEY(tenant_id,account_id),
    FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
    FOREIGN KEY(provider_id,revision,model,effort)
        REFERENCES provider_model_probes(provider_id,revision,model,effort)
) STRICT;
PRAGMA user_version=86;
