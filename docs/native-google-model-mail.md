# Native Gmail model reads

This is a separate opt-in native read adapter. The existing INBOX counts,
preview and selected-body grants keep their original behavior. Their stored
projections are not accepted as model-readable source selections.

The trusted companion supports `select-google-model-read ITEM_UUID GOOGLE_SUB
EMAIL` for foreground model reads. `select-google-model-background` is a separate
explicit choice that also permits one-off and recurring background reads while
the trusted device is online and its vault is unlocked. These commands authorize
sending the selected mail content to the configured LLM. They do not authorize
sending email. Compatible reads reuse the local grant without per-message prompts.

The implemented device adapter refreshes its selected OAuth credential, checks
actual readonly/OpenID/email scopes, verifies the selected Google subject and
verified email, and compares the Gmail mailbox profile before every read. It
supports bounded Gmail search and opaque page cursors, selected message/thread
plain-text projections, and selected PDF/UTF-8 attachment text pages. Search
returns at most five messages; a thread projection returns at most five messages
and reports omitted content. No arbitrary destination or request URL is accepted.

Provider message/thread/part IDs, provider cursors, refresh/access tokens, OAuth
client values and the account-selection identity stay on the device. The private
local journal maps fresh opaque UUIDs to those provider IDs under the exact grant
binding and expiry. Returned message observations and content digests identify
what was actually read. Follow-up selections cannot cross grant versions,
devices, withdrawal or expiry. The backend also pins selector kind, original
search query/background mode and the complete grant/credential revision. Invalid
or stale selections produce a bounded request error without suspending a healthy
local grant; actual scope/account/credential failure still requires reconnection.
The device checks selector lineage before OAuth or Gmail requests. Attachment pagination pins the first content
digest and rejects a changed source rather than mixing revisions.

Only PDF and UTF-8 plain text attachments are supported. Files are bounded to
2 MiB and extracted text to24 KiB, with at most50 parser pages; each returned text
chunk is at most4096 UTF-8 bytes. PDF extraction requires the explicitly configured
absolute trusted local executable in `--pdf-extractor`, supporting the
`internal-pdf-text` child protocol. The companion binary itself implements that
protocol. The parser runs under the existing CPU/memory/descriptor/time bounds.
Without the configured parser the result is `parser_unavailable`; unsupported
formats and missing mail have separate typed results. Attachment chunks are text
chunks and do not claim original PDF page-number fidelity.

The model projection is rebuilt from typed fields, bounded to12000 encoded bytes,
and signed under a new result type. Search snippets/thread text are bounded more
narrowly to keep the complete signed result within that cap. All original values
that can become projected content are checked for the currently used credential
values before truncation. This is not automatic removal of arbitrary secrets in
email content: the owner has authorized this content's LLM use.

Protocol operation8 and projection9 are separate from all previous operations.
Registration21/refresh22 record the native model-read/background descriptor;
payload lookup23 binds the exact invocation and request digest. The device checks
the typed payload digest before provider execution. Existing signed request and
projection encodings remain byte-compatible and have independent legacy vectors.

The Store backend supports registering this separate grant, admitting a request
to an exact owner/device/binding, returning its credential-free payload to that
paired device, and retrieving its authenticated signed result. Withdrawal removes
access to the result. Model/context/mission use must additionally be admitted by
an actually probed, pinned native-mail tool profile. Both native model-read profile
flags default to false; model names do not enable them.

A build with the existing default-off `credential-broker-chat` feature admits a
separate version1/operation8 unresolved native slot only in a fresh run admitted with the selected model's actual saved tool proof. Existing profiles remain off. Current chat uses `native_mail_search`,
`native_mail_message`, `native_mail_thread` and `native_mail_attachment`. An explicit
one-off task such as “Read native Gmail and summarize the invoice” can use the
same tools; durable/background tasks additionally require the device's separate
background grant. A missing grant or locked/offline device leaves the original
card waiting. A single compatible grant is reused, while multiple grants require
one precise owner choice. Device/account addresses are not copied into discovery;
owner-written aliases and opaque device/binding IDs identify choices. An explicit
mailbox address in an account phrase cannot be inferred from a sole opaque grant.

Native task requests use the actual supervised run and model step. A durable
mapping binds each original provider call ID to its one broker call UUID and
nonce; there are no random shadow runs. Admission pins the selected device and
complete grant, and later reads cannot fail over to another device. Opaque
message/thread/attachment/cursor selections must come from an authenticated native
read in the same task and grant. Stored display previews and body selections do
not qualify.

The worker yields before native dispatch and returns its lease. The ordinary DB
loop and signed device events adopt the exact bounded signed result into the
original typed tool slot, then resume the same run with a fresh lease and its
saved continuation. Waiting performs no model calls. Original model/tool/repair
counts, accepted revision and deadline are preserved: ordinary chat retains120
seconds from first dispatch, missions retain their original limit of at most one
day, and native grant/operation expiry can only shorten a wait. Ready missions use
the shared oldest-first owner-slot promotion; explicit pause is preserved.

A device commitment remains distinct from an undispatched wait. Signed unknown
or expired committed reads retain unknown/reconciliation state and never reenter
inference automatically. Late authenticated receipts may be recorded without
replaying a model or provider operation. Device/grant withdrawal purges dependent
run context through the existing artifact/learned-proof invalidation path.
Restoration quarantines native task payloads, selections, grants and derived
context rather than renewing device authority.

`native_task_mail::report_receipt` exposes an exact signed Message/Thread/Attachment
source address to the requested-report verifier. It rejects search snippets,
display results, unsupported/parser-unavailable results, changed grants and
withdrawn receipts. The report verifier must explicitly consume this helper; a
model's JSON provenance fields alone are insufficient.

Focused synthetic Store checks exercise actual signed broker commands and real
run/step slots for background completion after logout, foreground account choice,
same-run search→body follow-up, source provenance/quarantine, revocation,
undispatched/committed expiry, signed unknown outcomes, old-profile rejection and
legacy-selector exclusion. A real Runtime with a loopback model verifies current
chat and a one-off background mission wait without model polling, receive their
signed result and finish through their next original model step. The native HTTP
adapter tests below are separate from these supervised-runtime tests.

Synthetic tests exercise actual loopback token/identity/mail HTTP, selected text
and attachment pagination, exact grant/expiry/owner/device/digest boundaries,
background denial before provider calls, account/scope mismatch, current-credential
echo rejection, parser unavailability/local child projection, and legacy wire
vectors. They do not establish actual Google-account or off-host acceptance.

The supervised slot also reserves all native calls returned in one completed
model round before yielding. Partial signed replies do not reclaim the run; all
original native slots must settle before the single continuation. Other saved
local/connected calls in that round execute only after the run's guarded reclaim.
Cancellation before dispatch retires the request. Cancellation after commitment
keeps the conservative unknown outcome and never enables an automatic retry.

The selector-lineage regression includes a real local Engine: a signed search,
wrong-query cursor, wrong selector kind, stale credential-version selection and
then a valid fresh read. Request errors make no OAuth/Gmail request and do not
suspend the grant. Independent scope/account failures still suspend it until
local reconnect. Backend settings admission and supervised slots use the same
operation/query lineage helper without changing signed wire bytes.

Local verification on2026-11-15: the seven supervised Store outcomes, the settings
selector-origin outcome, the signed Engine selector/rotation regression and the
real Runtime current-chat/background-mission outcome passed. Store/agent
all-target Clippy passed with warnings denied. The two focused wait-card UI files
passed20 tests and TypeScript checking passed. These are scoped synthetic results,
not real Google-account, trusted-device deployment or complete release acceptance.

The2026-11-15 follow-up repair shares one explicit-mailbox phrase parser with
server-managed Gmail: `account EMAIL`, `EMAIL account` and `EMAIL 계정`, including
surrounding punctuation. Native grants remain opaque, so an explicit address that
cannot be verified does not auto-select a different sole grant. Sender filters
such as `from:EMAIL` remain search data. A new valid request can still use the
original authorized grant.

Settlement uses durable schema68 sweep positions for pending reads and binding
dependencies. Each writer sweep visits at most64 entries, wraps and persists its
position across restart; older missing connections do not monopolize subsequent
sweeps. The scoped signed Store regression retains72 older pending slots while a
later task prepares two native requests, adopts a signed receipt after reopening
at cursor64, and continues only when both original slots settle. Original task,
profile, call counters and deadline remain unchanged; no inference is dispatched
by settlement. The nine native task Store outcomes and the existing explicit
server-account outcome passed after this repair. Actual provider/device acceptance
remains outside these synthetic checks.
