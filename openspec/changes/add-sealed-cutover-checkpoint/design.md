# Seal and output authority

The FULL-sync active->sealed DB commit adopts retirement under the exclusive
existing writer lock. A redundant marker is an early additional fence. The same
lock transfers from private-copy DB/WAL admission into Store and account recovery;
sealed sources never reach runtime mutations, workers, key handling or enrollment.
Older active first-open roots retain their initialization/permission behavior only
after nonmutating admission. Ordinary setup admits before identity challenge.

Complete journal heads must equal applied heads before seal; no replay or provider
key is needed. The payload captures current SQLite withdrawal state, not an older
export advanced to newer heads. Only cutover bookkeeping changes after adoption.

Encrypted staging is finalized/fsynced before a ready receipt fixes its digest.
After ready, no re-encryption or new IDs are allowed. Missing/truncated matching
stage bytes can be repaired only from an exact already-published ciphertext; a
conflicting inode's bytes are never truncated. Ciphertext then head use owner-private
attempt temps and native no-replace rename, with file and parent fsync. Existing
matching bytes are re-synced; unknown/stale partials are preserved, not recursively
removed. Matching currently owned temp inode cleanup is best effort.

# Trust and limits

Distinct six-entry purpose assistant-sealed-cutover and bounded closed seal/head/
manifest codecs preserve legacy export formats. The supplied head is not inherently
trusted or latest. Local complete means local durable outputs only. Independent
manual trust/copy and future restore quarantine/enrollment/accounting remain outside
this unit. Artifact16 capture and future schedules/lifecycle restore quarantine are
explicit integration gates; schema numbers do not imply their completion.
