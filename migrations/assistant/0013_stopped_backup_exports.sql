-- Only public backup recipients and persistent installation binding. No identity.
CREATE TABLE backup_key_epochs (
 epoch INTEGER PRIMARY KEY CHECK(epoch>0),
 recipient TEXT NOT NULL CHECK(length(recipient) BETWEEN 1 AND 128),
 verified_at INTEGER NOT NULL
) STRICT;
CREATE TABLE backup_installation (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 installation_id TEXT NOT NULL UNIQUE CHECK(length(installation_id)=36),
 active_epoch INTEGER REFERENCES backup_key_epochs(epoch)
) STRICT;
INSERT INTO backup_installation(singleton,installation_id)
SELECT 1,lower(substr(value,1,8)||'-'||substr(value,9,4)||'-'||substr(value,13,4)||'-'||substr(value,17,4)||'-'||substr(value,21,12))
FROM (SELECT hex(randomblob(16)) AS value);
PRAGMA user_version=13;
