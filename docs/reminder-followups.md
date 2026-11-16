# Missing-field replies for fixed reminders

An explicit current-user reminder creation request can now ask for its missing
field and use the immediately next answer in the same conversation. It creates
only a fixed-text local inbox reminder, never a model task, mail, browser action
or provider call. There is no extra approval screen.

Examples:

- `물 마시라고 알려줘` → “언제 알려드릴까요?” → `내일 오전 9시`.
- `remind me to stretch` → a time question → `in 10 minutes`.
- `내일 오전 9시에 알려줘` → a content question → `물 마시기`.
- `remind me in 10 minutes` → a content question → `내용: Review the report`.
- `내일 9시에 약 먹으라고 알려줘` → an ambiguity question → `오전`.

The existing [schedule contract](schedules.md) still determines occurrence,
timezone/DST, publication, owner grant, control/revision and restore behavior.
Complete supported commands retain their direct zero-model admission path.
Unsupported reminder languages/forms are not converted by a model classifier.

## Bounded creation draft

Schema51 adds a private typed `schedule_chat_drafts` sidecar, separate from actual
saved schedule receipts and from Calendar/mission request continuations. It stores
only literal provided content, a parsed rule or incomplete time specification,
source IANA timezone, reference, expiry and answer count. A question creates no
schedule/background authority. State and the question commit in the same existing
chat submission transaction.

Only the immediately next chat sequence of the same tenant/account/thread,
original authenticated session digest and account epoch may use a pending draft.
The source turn must have completed. The draft lasts15minutes from the initiating
request and permits at most two answers. It is consumed when the next answer is
considered, including when that answer is unrelated or starts an independent
command. It cannot be rediscovered through general history, another thread,
another login session, a selected old receipt or model output.

An independently recognized complete reminder command uses its own current fields
and takes precedence; an incomplete new explicit creation starts a fresh draft.
A selected artifact/mission/reminder target cannot fill the old creation draft.
Exact cancellation replies `취소`, `취소해줘`, `그만`, `아니요`, `cancel`,
`cancel reminder`, `never mind`, `nevermind`, or `stop` discard it. Korean
spacing/polite forms such as `취소해 주세요` and complete short replies such as
`됐어, 취소해 줘` also close only the pending draft without inference. With an eligible
draft, cancellation (also with a final period/exclamation mark) closes that draft
before existing saved-reminder deletion is considered; saved reminders stay intact. Questions,
translation/meta wrappers and new unrelated requests are not silently treated as
old reminder content. An unsupported reply may follow the ordinary chat path but
retires the prior draft; the next later answer cannot revive it. An expired draft
also supplies no authority.

Current or inherited media prevents creating or filling these drafts, matching
the existing attachment boundary. The separate pending state is disabled during
restore quarantine; an ordinary Store reopen preserves a valid pending draft.
Deleting its source conversation deletes the sidecar through its chat-run foreign
key. Session/epoch checks fence logout/relogin/account changes without fabricating
new authority from a draft.

## Time and literal content

If time was entirely missing, a complete supported bare time uses the answer's
saved admission-time reference and the original frozen IANA timezone. Thus an
answer `in 10 minutes` five minutes after the first question schedules ten minutes
from the answer, not five minutes from it. Reusing the same request UUID returns
the committed result rather than recalculating the clock.

If a time/rule was already supplied, it is resolved once and retained while the
content is requested. A civil partial time retains its original reference/date
constraints; only the missing component is supplied. Supported component answers
are AM/PM or 오전/오후 for a numeric clock lacking it, and today/tomorrow or
오늘/내일/모레 for an otherwise explicit clock lacking its day. A first bare
incomplete time may ask for its final component within the two-answer limit. ISO
dates retain their date when a Korean AM/PM answer supplies the missing marker.
If the second answer still leaves a field missing, the draft closes and asks for
a complete new request instead of asking an unusable third question.
Supplied content/day/clock is never replaced with conflicting full-answer fields.
Ambiguous unsupported fragments, Korean word-number partial clocks, missing source
timezones for civil time and DST gaps require a new complete request rather than
invented interpretations. An elapsed one-shot time that has passed while waiting
for content is rejected; it is not moved forward to a newly guessed time.

For missing content, arbitrary bounded literal text is accepted after an explicit
`내용: `, `내용은 `, `message: ` or `text: ` label (maximum2000 UTF-8 bytes, existing
control-character policy). Literal payload is saved and never executed, including
quoted/meta/negative words inside that explicit content. A small bare-answer
language supports `stretch`, `drink water`, `take medicine`, `walk`, `water plants`,
short `review …`, `finish …`, `practice …` phrases, or short Korean phrases ending
in the nominal 기 form (e.g. 우산 챙기기, 반품 접수하기, 약 타오기), or 스트레칭.
These contain at most six alphanumeric words, spaces or hyphens; time phrases,
new command clauses and conditionals still do not fill a literal activity. Bare answers are capped at160bytes
and bounded word count and exclude question/quote/command punctuation. Outer
translation, summary, explanation, quotation, source, search and send prefixes
are excluded before the bare suffix grammar; explicitly labelled content stays
literal, including those words.

This deliberately does not claim semantic detection of whether all arbitrary
prose is related to the prior question. Unrecognized prose is not an implicit
content grant; use an explicit label for other literal reminder text. A draft is
for creation only and grants no general write/action authority.

## Atomic outcome and verification

Completing a draft uses existing `create_in`, current owner validation, exact
request-id idempotency, schedule receipt and final chat confirmation in one SQLite
transaction. Failure rolls back the new schedule, consumed-draft flag and chat
turn together. The saved receipt contains the actual schedule identity/revision
and frozen timezone. Identical retry/reopen returns it once; changed payload under
the same request UUID conflicts. Existing schedule publication and cancellation
semantics remain unchanged.

Focused actual Store checks cover reopen plus exact retry and clock/zone freezing;
missing content and partial AM/PM/day preservation; cancellation/unrelated/new
command, session/thread/owner/TTL and restore fences; the shared SQL-prefix migration sweep,
transaction rollback at question and final receipt, and deletion/FK integrity.
The existing current/inherited image integration is extended to incomplete requests
and their time answers. Evidence is recorded in the
[dated review](reviews/2026-11-15-reminder-followups.md). These bounded language
forms and synthetic Store outcomes do not certify all natural-language reminders,
real mobile timezones/notification permissions or full P5 completion.
