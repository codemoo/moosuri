# Design

The Store seals original validated intent, nonce, generation and the complete
bounded current candidate snapshot before a second remote dispatch. Public candidate
views contain indexes and assertion metadata; private memory IDs/revisions stay in
the snapshot. Only a completed closed index/missing/ambiguous result is accepted.
This selection is semantic judgment, never new action or fact authority.

All effective-version rows are enumerated in stable order. More than64 scanned
rows or32KiB of data clarifies; no keyword-selected partial list authorizes writes.
A snapshot compares complete candidate data and separate effective/head pins at
selection and mutation, as well as the knowledge generation. Time drift, new rows,
changed rows and invalidations fence rather than silently reselecting.

Every candidate effective-memory and evidence-source revision is registered as a
knowledge dependency and the execution generation is pinned before sealing returns.
Forget/source withdrawal purges the private plan and checkpoints, including
unselected candidates. Resume and dispatch admission revalidate the snapshot before
any new provider request; in-flight admitted calls keep existing cancellation rules.

A private target-plan table survives explicit retry and is removed on finalization,
chat deletion and knowledge purge. A durable started flag prevents purged plans
from re-entering intent classification or preparation. Selection calls share the
six remote calls, twelve local tools and120-second ledger. No remote wait holds the
delivery gate. Adopted forgetting keeps its existing independent journal contract.

Integration note: the isolated worker applies migration0012 directly after0010 with
local user_version11. Main must apply community0011 first, then set this migration
and its lib.rs expectations to12. No fake0011 is included.

Synthetic decisions prove authority, lifetime and concurrency behavior, not model
understanding. Independent live cross-topic and injection evaluation remains needed.
