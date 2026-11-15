# One explicit direct read

`parcel_history` maps to WebToolRequest::ParcelHistory. It uses the existing
external slot, dispatch nonce, budget, cancellation and owner/read fences. One
fixed HTTPS direct GET has reqCodeNo1 and locale=ja, without a summary preflight,
retry or fallback. Current bundled1.1.0 exact revision/digest gates Store admission;
old1.0.0 profiles do not gain history vocabulary or dispatch authority. Existing
exact current pin policy also fences their summary reads; no dual legacy authority
is retained. A fresh current-pin run/conversation is needed after the update.

The existing Store resolver reuses unique actual receipts in the same owned thread
within32turns, preserving original number source and current request identities.
A defaulted history flag in private origin metadata validates the exact direct URL
at result commit; no DB schema changes. Summary and history may supply identity
for later reads but neither supplies current status without new IO.

Published scraper revisions and official explanatory history tables provide
structural evidence. Current all-zero official direct not-found shape was read;
no private or real tracking identifier was used. See docs/parcel-tracking.md for
exact source links and limits. Synthetic fixtures use newly authored values.

The parser requires one form-bound detail table matching the requested number,
one history table, two exact supported ordered header rows and paired5/1-cell
event/postcode rows. Supported Japanese header labels/spans come only from the
official explanatory tables cited in docs/parcel-tracking.md; unseen aliases fail.
All event cells must be td, columns0/1/2/4 span2rows, office/postcode span1, and all
colspans are1. Unknown/reordered labels, extra th/td or broken spans fail before
positional mapping. Current live positive header equivalence remains unaccepted. A
malformed/duplicate/mismatched/unsupported table yields history_unavailable and no
events. It preserves literal fields/order, normalizing only explicit year dates
and explicit RFC3339 offset times. Caps:1MiB input,64events,512bytes per field,
12KiB serialized result to fit existing16KiB receipt/model limits with provenance.
Oversize fails rather than silently dropping events. official_visible_history
covers supported rows in that response, not globally complete carrier records.

Existing parcel_track and private japan-post-latest-v1 learning remain summary
only. A history receipt is not a new learned route or a summary learning proof.
Artifact-write stages continue excluding parcel tools; no new report provenance
or mission capability is implied. Combined provider definitions keep all tools.
