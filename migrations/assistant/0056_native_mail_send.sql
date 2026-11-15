-- Explicit transport and immutable one-device adoption for existing approved mail.
ALTER TABLE mail_connections ADD COLUMN native_send INTEGER NOT NULL DEFAULT 0 CHECK(native_send IN(0,1));
CREATE TABLE native_mail_connections (
 tenant_id TEXT NOT NULL,connection_id TEXT NOT NULL,device_id TEXT NOT NULL,binding_id TEXT NOT NULL,auth_epoch INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,connection_id),UNIQUE(device_id,binding_id),
 FOREIGN KEY(tenant_id,connection_id) REFERENCES mail_connections(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(device_id,binding_id) REFERENCES credential_broker_bindings(device_id,binding_id)
) STRICT;
CREATE TABLE native_mail_effects (
 tenant_id TEXT NOT NULL,action_id TEXT NOT NULL,revision INTEGER NOT NULL,effect_id TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN('send','lookup')),
 device_id TEXT NOT NULL,request TEXT NOT NULL,payload TEXT NOT NULL,lease_nonce BLOB,
 PRIMARY KEY(tenant_id,effect_id),
 FOREIGN KEY(tenant_id,action_id) REFERENCES mail_actions(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX native_mail_send_once ON native_mail_effects(tenant_id,action_id,revision) WHERE kind='send';
CREATE TRIGGER native_mail_effect_identity BEFORE UPDATE ON native_mail_effects
WHEN NEW.tenant_id!=OLD.tenant_id OR NEW.action_id!=OLD.action_id OR NEW.revision!=OLD.revision OR NEW.effect_id!=OLD.effect_id OR NEW.kind!=OLD.kind OR NEW.device_id!=OLD.device_id OR NEW.payload!=OLD.payload OR NEW.lease_nonce IS NOT OLD.lease_nonce
BEGIN SELECT RAISE(ABORT,'immutable adopted native mail identity'); END;
PRAGMA user_version=56;
