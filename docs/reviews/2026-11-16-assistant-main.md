# Main assistant defects and follow-up evaluation — 2026-11-16

This pass followed ordinary conversations, current-memory lookup and edits,
source calculations, attached PDF text, mission completion/recovery, and the
browser actions that control those runs. Inputs, accounts and data were synthetic.
No production service, private collector, real mailbox or operating database changed.

## User-visible repairs

- Current-memory target enumeration excludes expired effective revisions before
  the coverage limit, while never reviving an older superseded assertion.
- Ordinary chat has a trusted original-message date/timezone reference, fixed on
  retry. The leading instruction preserves it across tool/media profiles and all
  four provider dialects; missing timezones use an explicit UTC fallback.
- Narrow read-only memory and ordinary answer-continuation forms avoid a separate
  mutation/task classifier. Mixed edits, named tasks, selected targets and unknown
  forms preserve their existing routes.
- Source-bound exact calculation accepts one colon, full-width colon or equals
  separator after the literal field label. It still requires one current unique
  source, exact source byte evidence and current-request numeric operands.
- Media instructions now describe the existing extracted-PDF-text capability.
  PDF extraction was already working in the baseline; this fixes a contradictory
  instruction, not an observed baseline PDF task failure.
- Incomplete/refused mission rounds cannot become candidates, private reports,
  input waits or tool authority. Legacy pending output without verified terminal
  metadata cannot replay after restart; existing receipts and budgets remain.
- Retry, cancel, send and conversation deletion use the actual current run or
  deletion identity. Confirmed state releases a hanging HTTP receipt; late replies
  cannot unlock or overwrite a newer action. Old failed turns stop offering Retry
  after a later turn exists, and deletion preserves the latest inventory/drafts.

Detailed bounds and reproduction evidence are recorded in the
[chat](2026-11-16-chat-main.md), [memory/context](2026-11-16-memory-context.md)
and [mission terminal](2026-11-16-mission-terminal-admission.md) reviews.

## Baseline observations and evaluation limits

The initial ten new development requests used gpt-6.1-terra/high through the same
private official-sign-in evaluation bridge as earlier development checks. They
included memory creation/correction/recall/deletion, ordinary follow-up, source
arithmetic, conflicting sources and a checklist explicitly excluding schedules.
The bridge exposed only the product's bounded dynamic tools. No native shell,
external connector, real-mail action or credential export was enabled.

All ten produced semantically suitable final text, but that is not a strict10/10
claim: the original recall oracle asked for a full memory read, while the model
used current owned search snippets. The original oracle and all observations are
preserved privately. Current-source calculation first rejected `재료 단가: 13.75`
when asked for the literal label `재료 단가`; a manual read/arithmetic fallback
reached62USD using five model calls. Ordinary follow-up used two model calls.

The baseline PDF upload/extractor read and same-thread follow-up both returned
AX-731 and46.25USD from extracted text, despite the old contradictory prompt.
An earlier harness upload-method mistake returned405 before any chat task; it is
retained separately and is not a model task result.

## Integration evidence

Linux source-field execution passed10 cases and the combined tool preflight3.
Integrated Store chat/context passed55 cases, memory commands39, runner projection4
and the final-description combined tool preflight3. Two old synthetic schema
fixtures initially failed after downgrading a current database without removing
newer tables; only those fixtures were repaired, and the55-case run passed.
Production migration code was not changed.

The frontend author passed111 tests across six affected suites and the production
build. The later confirmed-deletion receipt fix passed seven focused deletion
observations plus app/worker type checks. These filters overlap; counts are not a
sum of unique tests or a quality score.

Independent review found and closed mixed memory-correction routing, named-task
continuation routing, legacy pending mission recovery and confirmed deletion locks.
Source delimiter/PDF and date/context projection review found no additional blocker.

Linux mission Runtime passed10 tests (including nine terminal variants and three
legacy-pending boundaries), Store mission/recurring tests74 and memory intent9.
The final generic-reference helper passed its nonzero core test and all six actual
mission-chat Runtime tests. A first strict lint run rejected a production helper
placed after a test module; moving the unchanged helper before tests resolved it.
Final affected-crate strict Clippy and the embedded Linux CLI build passed.
Native embedded CLI and final web production builds also passed.

## Unchanged-request live recheck

The candidate reran the first eight original requests without modifying their
inputs or expected criteria, then added an ordinary Seoul date question. Native
returned model/effort metadata corroborated gpt-6.1-terra/high for all18 starts,
including setup. Memory creation/correction/deletion preserved the same record
identity and expected revisions, and the other owner's91-minute memory was
unchanged. The date answer matched the stored original-message Seoul date.

| Request | Model calls before → after | Observed seconds before → after |
| --- | --- | --- |
| Ordinary explanation control | 1 → 1 | 3.36 → 4.01 |
| Referencing the preceding explanation | 2 → 1 | 8.58 → 3.62 |
| Current memory recall | 3 → 2 | 17.40 → 8.25 |
| Recall after deletion | 4 → 3 | 19.67 → 11.36 |
| Current-source exact calculation | 5 → 3 | 20.65 → 12.00 |

The calculation returned62USD through a successful bound `source_calculate`
receipt with the exact13.75 source span and current-request4/7 operands. It still
performed a preceding search, so the observed result is three calls, not two.
Recall again used correct current owned search hits; the original full-read
oracle remains unmet. Conflicting-source and no-schedule baseline cases were not
rerun. These are single exposed development comparisons, not a holdout score,
latency distribution, guaranteed percentage gain or production performance claim.

Both unchanged PDF requests completed in one call each, returning AX-731 and
46.25USD with the matching filename/revision/digest and extracted-text span[9,47).
Native metadata corroborated Terra/high. This also verifies same-thread attachment
context without attaching the PDF again.

Two candidate setup failures are retained separately. First, the sandbox prevented
the official CLI opening its own local state DB before any chat; the authorized
harness then used a fresh synthetic world. Later the PDF world's tool probe repeated
a ping instead of ending, again before any chat. Its successful text probe was used
without repeating the tool probe; PDF media conversations disable tools by contract.
The two successful PDF chats do not establish successful function-tool probing in
that world. No credentials or raw traces enter the repository.

## Actual browser recovery

An isolated Chrome profile used the freshly embedded application and a loopback
synthetic provider. A real retry reached the backend while the browser deliberately
held its successful HTTP response. Canonical SSE attempt2 aborted the redundant
wait after30ms. Cancel then sent expected_attempt2 exactly once, and a new message
completed. Stored history had one cancelled run at attempt2 and one later completed
run; the older retry control was absent.

A real DELETE with a deliberately held204 response was confirmed through the
selected thread's SSE closure. The matching wait ended after15ms, the dialog closed
and another conversation's Delete control remained available. Deleting that other,
unselected thread with another held receipt used the30-second deadline, then one
GET inventory reconciliation closed the dialog. Exactly one DELETE was sent per
thread, with no automatic mutation replay and no remaining conversation rows.

Independent review accepted the final deletion/action owners, corrected mixed
memory clauses, source evidence binding, original-message context, legacy mission
checkpoint guard and final shared continuation helper. Public preflight covers
formatting, plan/OpenSpec metadata and source/history secret scans. CI remains
disabled. This change does not deploy or establish general assistant completeness.
