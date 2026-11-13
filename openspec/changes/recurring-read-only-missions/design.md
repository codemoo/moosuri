## Authority and admission

The current authenticated owner saves an immutable MissionCreate template and a ScheduleDraft. The server captures the verified provider revision/model/effort and admitted read-only tool profile; browser/mail pins are removed as in one-time missions. Polling has no UserContext and never resolves current defaults. It passes primitive pinned grant fields into the common mission insertion transaction.

A scheduler sidecar reuses the existing IANA/DST/interval/catchup oracle, revision and epoch fences, unique UTC/local-slot receipts, latest-only queue and restore quarantine. Generic reminder listing/lookup/chat selection/mutation/claim/publication cannot access it. Recurring polling selects at most one queued occurrence per owner, ordered by the shared persistent fairness counter, and considers at most64owners per tick. Existing owner/global admission and runner concurrency limits continue to apply. Busy slots remain bounded by the scheduler's one latest queued occurrence.

Insertion of thread, run, mission, execution budget, occurrence link and admitted marker is one SQLite writer transaction. Repeating or rolling back a poll cannot authorize duplicate missions. Deadline is min(scheduled UTC+24hours, configured validity end), rather than admission+24hours. Expired one-shots are observable skipped/expired receipts without a fresh model budget. Original six-model/twelve-tool/two-repair limits remain in the mission carrier.

## Controls and recovery

Parent epoch/revision/state are checked in each mission execution guard as well as background admission/publication. Pause/edit/revoke retire unfinished children, erase uncommitted candidates/private proposals, invalidate execution and retain observed usage/receipts. A dispatched child becomes unknown, not replayable. Resume only permits new future occurrences. Idempotent control retries do not retire children admitted after the original control. No provider/model fallback or automatic epoch renewal occurs. Sealed restore pauses parent schedules and quarantines unfinished missions; reenrollment alone does not resume them.

## Results and equality

Terminal records stay in actual mission history. Local notices contain only a fixed title, internal terminal status and owner-authenticated mission link. No template text, source title/quote or model output is copied into immutable notification text. Existing generic Push continues its separate device/account policy.

Changes-only compares exact candidate text plus a sorted/deduplicated set of actual completed decimal and source receipts. Source ID, revision and returned text are retained; call IDs, step IDs and observation times are excluded. Other evidence types are incomparable and notify. Only achieved, nonuncertain candidates qualify. The immediately previous occurrence in the same schedule revision and auth epoch must also qualify; failure, unknown, review, withdrawal or a control/epoch barrier cannot be silently reused as a successful baseline. This is exact-content equality, not semantic comparison or a proof of arbitrary prose. Execution history always records suppressed notices.

## Remaining scope

Conversational mission scheduling is not connected; literal reminder capture remains bounded and separate. General memory/search/web/skill result normalization, semantic comparison, actual-device Push checks and live-provider performance/quality are not demonstrated by this synthetic slice. No mail/browser/vault/external-write authority is added.
