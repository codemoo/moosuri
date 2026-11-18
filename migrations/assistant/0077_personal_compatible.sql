-- Ownership is immutable and never inferred from the selecting account.
CREATE TABLE personal_compatible_providers (
 provider_id TEXT PRIMARY KEY NOT NULL REFERENCES providers(id),
 tenant_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TRIGGER personal_compatible_owner_immutable BEFORE UPDATE ON personal_compatible_providers
BEGIN SELECT RAISE(ABORT,'immutable provider owner'); END;
CREATE TRIGGER personal_compatible_owner_no_delete BEFORE DELETE ON personal_compatible_providers
BEGIN SELECT RAISE(ABORT,'immutable provider owner'); END;
-- Account deletion may redact private revisions, retaining only opaque IDs.
DROP TRIGGER provider_revisions_no_update;
CREATE TRIGGER provider_revisions_no_update BEFORE UPDATE ON provider_revisions
WHEN NOT (
    NEW.provider_id IS OLD.provider_id AND NEW.revision IS OLD.revision
    AND NEW.name IS OLD.name AND NEW.config IS OLD.config AND NEW.enabled IS OLD.enabled
    AND OLD.credential IS NOT NULL AND NEW.credential IS NULL
    AND OLD.revision != (SELECT current_revision FROM providers WHERE id=OLD.provider_id)
) AND NOT (
    NEW.provider_id IS OLD.provider_id AND NEW.revision IS OLD.revision
    AND NEW.name='Deleted personal connection' AND NEW.config='{"kind":"compatible","base_url":null,"allowed_ips":[],"router_providers":[],"zero_data_retention":false}'
    AND NEW.enabled=0 AND NEW.credential IS NULL
    AND EXISTS(SELECT 1 FROM personal_compatible_providers o JOIN accounts a ON a.id=o.account_id AND a.tenant_id=o.tenant_id WHERE o.provider_id=OLD.provider_id AND a.deleted=1)
)
BEGIN SELECT RAISE(ABORT,'immutable revision'); END;
PRAGMA user_version=77;
