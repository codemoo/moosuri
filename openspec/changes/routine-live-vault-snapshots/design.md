## Writer and immutable capture

HTTP and the opt-in worker share one Arc/Mutex Database. Under writer exclusion the
capture checks ready/meta sequence/head, bounded DB pages and source admission,
copies SQLite through Backup and copies the exact journal with an independent
descriptor. No live append offset or source authority changes. Semantic replay,
encryption and publication consume a private immutable stage outside that mutex.

## Format and recovery

Live snapshot uses moosuri-vault-live-snapshot/MVLS0001 while retaining the bounded
checkpoint-binding/file framing shape. The sealed-current format/magic remain
unchanged and accepted only by their original restore entry. Both share strict
age/file/head/schema/semantic checks and the same quarantine/publication rules.
Snapshot restore assigns a new service UUID, cancels pending operations, removes
sessions and preserves already captured tombstones/history/reservations. Repeated
exact restore preserves subsequent destination work. No HTTP export route exists.

## Limits and gaps

Sequential attempts are immediate then300..604800seconds after completion. Existing
private filesystem/size bounds apply; capture checks60seconds between operations,
not a hard blocking-IO deadline. SIGINT/SIGTERM signals stop and drains the admitted
job. A receipt pins captured bytes/head only. No new journal transport/independent
latestness evidence exists; post-capture deletion/nonce use cannot be reconstructed.
Restoring an explicit older snapshot may recover older ciphertext. That boundary
is named, tested and documented; no silent historical-forward schema is introduced.
