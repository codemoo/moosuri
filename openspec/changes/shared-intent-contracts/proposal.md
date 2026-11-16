# Shared current-intent contracts

Calendar's model schema allowed arbitrary clarification prose and operation names,
while continuation persisted only finite machine fields and Store accepted only
create/update/delete. A schema-valid prose clarification produced a generic prompt
without an answerable pending field. Calendar/artifact parsers also accepted omitted
nullable keys that their schemas declared required; memory already enforced them.

Use shared finite vocabularies and exact current-model key checks. Keep null literal,
retain persisted Calendar replay compatibility, and preserve all existing explicit
current-user, account, source, quote, target and effect guards. This is a P2/P4/P6
contract repair, not new write or approval authority.
