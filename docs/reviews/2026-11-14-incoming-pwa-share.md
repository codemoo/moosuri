# Incoming PWA text share implementation evidence

2026-11-14. Bounded F16 text/title/URL implementation accepted after independent R3
review and integration with personal ChatGPT/audio. Actual OS-installed/mobile
acceptance, file/audio share and aggregate F16/P7 remain incomplete.

The dedicated5min plaintext IDB handoff is separate from public Cache API and never
queues API mutations. Explicit review/insert and normal Send remain distinct. One
window claims the payload; consume erases before append and retains a tombstone.
A crash after consume can lose text; no automatic claim transfer is promised. New
intentional share replaces an abandoned claim, with at most32 payload-free tombstones
until original TTL. Closed-browser timed disk deletion is not guaranteed.

Initial independent review reproduced remote revocation being mistaken for ordinary
expiry and another context clearing IDB before a delayed claim callback disclosed old
text. R2 preserves the original nonbearer scope through401, including post-expiry
intake. A fresh owner-authenticated POST proves the original unrestricted session row
still exists in the current account epoch. Natural expiry retains rows; individual
logout/admin revoke/password/suspend/recovery deletes or epochs them. Exact tenant/
account proof has no admin bypass, scans at most257 owner rows and fails closed above
256 without renewing the origin. The shared hash helper preserves the existing chat
namespace. There is no schema, dependency or new approval workflow.

After delayed claim/consume completion, fresh session/origin proof precedes a final
IDB transaction checking durable generation/account/session/claim. Only its synchronous
callback writes review/draft UI; no delayed oncomplete snapshot authorizes a write.
Independent VM contexts sharing IDB without broadcast reproduced both original UI
failures and passed the repair. Optional retirement hints immediately hide displayed
review; auth retirement is announced before the logout cookie request completes.

R2 independent review accepted both P1 repairs and reproduced a narrower same-account
cookie race: final session GET observed A, origin POST authenticated new cookie B,
and account-only proof could still disclose/insert claim A. R3 requires private POST
fields origin_scope and expected_current_scope, exactly matching the latter to the
actual authenticated UserContext namespace under the server guard. All disclosure,
preconsume and postconsume proofs use this assertion. Original unclaimed A explicitly
imported as a fresh B claim remains valid; a claim pinned to A cannot migrate to B.
Cookie-switch UI disclosure/postconsume cases failed against R2 and passed R3. Both
original freezes, independent probes and actual RED logs remain retained privately.

Observed final frontend checks:80 focused tests passed across state/intake/review,
Push state/worker, App and ChatPanel. Cases cover independent-context delayed claim/
consume, held authority response, cookie replacement, unclaimed renewal continuity,
remote-origin refusal, retirement hiding, post-expiry provenance, original-slot
migration, bounded multipart/UTF-8/file/duplicate rejection, timeout/storage denial,
one-use/replay and zero chat submission. Frontend/worker TypeScript and production
Vite build passed. Actual17,394-byte emitted worker VM passed anonymous public-only
cache, real multipart/exact navigation POST, opaque no-store redirect, no URL fetch,
persisted IDB after worker context restart, duplicate rejection/account rotation and
retained Push/update/no-skipWaiting checks. Existing app chunk-size advisory remains.
Pinned strict OpenSpec passed.

Rust1.89 native checks passed Store5, actual HTTP2, Store/server all-target Clippy
with warnings denied and workspace formatting. Store covers natural expiry/no origin
renewal, targeted logout, password change/reset, admin EndSessions, suspend/resume,
cross-owner/admin denial, malformed/missing/old-epoch/restricted origin,256-row bound,
actual journal recovery/new login and hash compatibility. HTTP covers current owner/
header/body bound/no-store, logout/password revocation, exact current cookie pinning,
missing/malformed expected scope and allowed unclaimed same-owner continuity. Earlier
exact R2 Linux source passed Store5/HTTP1/all-target Clippy with source hashes verified.
No native or Linux case establishes live account/provider readiness.

## Integrated acceptance

Independent R3 review accepted the exact original/current namespace and cross-context
repairs. Root integrated both slot-migration callbacks and both audio/share composers;
server routing, schema33 and personal session behavior remain intact. Combined Linux
Store5, actual HTTP2 and affected all-target Clippy passed. Final React106, both TypeScript
checks and production build passed. The compiled worker runtime is byte-identical to
reviewed R3; its public-shell prefix additionally includes the voice WAV Worker asset.
The production Rust binary was rebuilt with the exact final UI and no fixture feature.

Actual Chrome390×844 used that Rust/SQLite server and a scripted local chat model.
An opaque-origin separate document submitted a real multipart navigation POST through
the registered worker. Only an opaque identifier appeared in the redirect. DevTools
stopped the actual active worker before claim; after reload, the committed handoff was
still available. Shared HTML/URL remained inert and caused zero external fetches.
Edited text appended once to the latest draft, without automatic Send; one subsequent
explicit Send produced one saved response. Reopening the consumed link failed, and a
second visible claim disappeared with its payload after logout in another tab. Private
API/share responses were absent from Cache API. The final themed review, compact
completion, current-composer focus and no horizontal overflow passed. Chrome reported
no installability errors. Both initial/final browser evidence and first failed review
reproductions remain in root's synthetic validation artifacts.

This is actual service-worker/browser protocol evidence, not OS share-picker or physical
Android/iOS acceptance. Full suspension/crash matrices remain separate. File/audio shares,
unbound401-before-login preservation and general private offline queues remain excluded
from this bounded implementation. No live provider credentials, operating data, CI or
production deployment were used.
