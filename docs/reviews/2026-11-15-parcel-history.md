# Japan Post visible history: bounded implementation evidence

F42/P6 adds `parcel_history` beside the existing latest-summary `parcel_track`.
It returns all supported visible rows on one official detail page with actual
source/time, while the old learned japan-post-latest-v1 route remains summary only.
No globally complete carrier record, current live positive-provider acceptance,
other carrier, prediction, notification or external write is claimed.

## Public structure evidence

On2026-11-15 public read-only research verified the official all-zero Japanese
direct request, exact not-found row/header shape, the existing English form and
its detailed-information transition, and official explanatory history tables.
The form's POST pathDetail without provider state returns an error; it is not used.
Three published implementations corroborate the fixed GET direct route and the
history structure: detail summary `配達状況詳細`, history summary `履歴情報`, two
header rows, followed by paired5-cell event and1-cell postcode rows. Exact links
and pinned revisions are in [parcel-tracking](../parcel-tracking.md). These are
structural sources, not proof of a successful current provider lookup. Parser
code was written independently. Fixtures contain only new synthetic values;
third-party shipment numbers/pages and private data are not committed.

## Execution and failure boundaries

The current bundled1.1.0 exact revision/digest grants Store history admission.
Old1.0.0 or stale-digest saved profiles cannot gain authority via crafted typed
history requests. The existing exact current pin policy also fences their summary
reads; no dual legacy authority is retained. Start a fresh run/conversation after
the bundled1.1.0 update. Summary result semantics and learned-v1 remain unchanged. Model vocabulary is separately version gated. Store supplies
literal/current or unique same-thread receipt identity, original and current
request provenance, and validates the exact direct URL when settling the real
external dispatch. A private defaulted origin flag distinguishes summary/history
URLs without a DB migration. One tool charges one existing dispatch/read; no
summary preflight, hidden fallback or retry is performed.

The bounded parser requires a matching exact item number and one known history
table. A bounded repair after independent review replaces nonempty-header checks
with exact ordered header semantics and spans. The official Japanese explanatory
page (linked in parcel-tracking.md) evidences only the two accepted label forms;
unseen current-detail aliases are not guessed and fail unavailable. Every event
row has exactly five td cells, rowspan2 on columns0/1/2/4, single office/postcode
cells and colspan1. Mixed th/td, extra headers, reordered/unknown labels and broken
spans fail before positional mapping. The actual current detail may use a different
unverified label form; current live positive acceptance remains open. It preserves date/status/details/office/region/postcode and provider order.
Missing year/offset remains missing. Unsupported/duplicate/mismatched/broken rows
produce history_unavailable with no events; a summary alone cannot become a
history success. Exact known Japanese not-found is distinct. Input1MiB,64events,
512bytes per field and serialized12KiB limits fit the existing16KiB tool result
contract with server provenance. Oversize fails instead of dropping events.
Overseas omissions/delay mean official_visible_history is page coverage only.
Existing session/read cancellation, lease, completed-slot reuse and unknown
external outcome rules remain in the shared dispatch path.

## Verification state

Integrated Linux acceptance passed the following actual tests. All filters ran
nonzero cases; the parcel Runtime filter also retained the existing learned-summary
execution and narrow argument/wire cases.

- Connector: `history_preserves_paired_rows_dates_and_identity_without_inferred_timezone` covers multi-event field pairing, raw missing-year/zone values, wrong number, malformed/duplicate table and event cap, reordered/unknown headers,
extra event th, missing rowspan and colspan mismatch.
- Agent: `parcel_tracking_actual_provider_tool_loop_reuses_only_same_thread_receipt_and_reports_changed_notfound` includes actual loopback direct HTTP→parser→tool→Runtime→SQLite/source receipts, two events, same-thread B provenance, one direct GET per history call, and honest refusal when only latest-summary markup returns.
- Store: `old_saved_pin_cannot_admit_history_even_with_crafted_typed_request` covers old revision, stale digest and current read revocation.
- Agent: `history_vocabulary_is_new_pin_only_and_typed_arguments_remain_narrow` covers old/new vocabulary and arbitrary URL rejection.
- Agent: `combined_read_profiles_reach_` exercises both plan and shared provider transports with27 definitions including both parcel tools; no existing tool is dropped.

The parser, Store admission and vocabulary cases each passed1; parcel Runtime and
learned-summary cases passed3; the combined provider transports passed2. Independent
review identified and accepted the exact-header/span repair before execution.
Affected all-target Clippy and the Linux assistant build passed. Pinned formatting,
plan metadata, strict OpenSpec and source/new-history secret checks run before push.

Integration also found that the older combined fixture omitted two existing Gmail
attachment tools. Retaining their actual definitions exposed unsupported range
keywords in the model-facing schemas. Removing those keywords fixes protocol
preflight while the typed/service bounds remain unchanged. Independent review
accepted that repair and both real HTTP continuation cases passed; see the
[combined-profile evidence](2026-11-14-combined-tool-profiles.md).

The new tests target defects that could falsely report full history, swap shipment
identity, infer an absolute timestamp, perform unreceipted extra reads or expand
old-run authority. Synthetic Runtime success is not broad model understanding or
real carrier stability. No learned route/export format was expanded. Current live
positive-provider markup remains unaccepted and may use a different label form.
