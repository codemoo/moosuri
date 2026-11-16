# Design

`schema.rs` owns ordered assistant version/SQL pairs and derives current version
from the last pair. Before returning a suffix it checks complete contiguous
coverage and the caller's explicit supported baseline. SQL bytes remain in their
existing migration files. A directory-driven test catches an unregistered SQL
file, including a forgotten final migration.

Store startup accepts schema0 through current. It retains independent immediate
transactions, the schema4 durable knowledge-journal barrier, schema15's durable
successor-journal/metadata transaction, the provider-save recovery between65/66,
and schema70's FK-off table replacement. Generic startup execution rejects15 so
it cannot bypass that handoff. Destructive-journal initialization and restored
cutover guards obtain their exact SQL from the same registry.

Restore accepts sealed schema19 through current. It executes registry SQL inside
quarantine's existing immediate transaction, without nested startup transactions
or journal initialization. Its pre-BEGIN FK policy, trusted_schema=OFF, final
version/integrity/FK checks, enrollment quarantine and staging publication stay
with restore. Registry reuse does not confer a caller's broader admission range.

Synthetic SQL-only predecessor sweeps verify migration coverage/shape and restore
rollback, not real historic binaries or encrypted archive provenance. Existing
current/predecessor encrypted restore tests cover the surrounding boundary.
