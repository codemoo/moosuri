# Local reminder schedules and notification inbox

`/app/schedules` supports account-owned fixed-content local reminders. Create a
title/message and one future UTC instant, anchored UTC interval (at least60seconds),
daily HH:MM, or selected ISO weekdays (Monday1 through Sunday7) in a chosen IANA timezone. The form can preview the interpreted next
time; save is the user action and does not require another approval screen. Edit,
pause/resume and delete use the current schedule revision. Up to64 nonrevoked
schedules are allowed per account; deleting releases a slot and preserves receipts.

This is the first F29/F30/F32 slice. A reminder delivers the saved text to the local
inbox; it does not execute the requested text as an agent task, send mail, run a
browser task or call a provider. Bounded conversational capture and weekly rules are implemented; actual
model tasks, changes-only result comparison (F31/AUTO03), topic mute and WebPush
remain next work. No browser notification permission is requested.

## Clock and occurrence policy

- Intervals keep the original UTC anchor+n×duration. Completing late does not move
  the anchor. Once inputs must be future UTC instants; late one-shots fire once
  only while their background authority and optional validity end remain valid.
- Daily and weekly rules retain their own IANA timezone, independent of later browser/default
  zone changes. A nonexistent local time produces a skipped local-slot receipt
  with no invented UTC execution. A repeated local time uses the first UTC offset
  once. Quiet hours use their separately chosen IANA timezone.
- A five-second supervised worker performs bounded DB polling. Idle model calls
  are zero. Each poll handles at most64 due schedules and32 local publications;
  persisted per-owner round-robin claim order prevents one owner taking all claims.
- Recurring downtime retains the latest valid missed occurrence. Older skipped
  local/UTC ranges and counts are summarized, rather than materializing years of
  backlog. The current gap has its own skip receipt; older coalesced ranges can
  include skipped gap dates. At most one claimed and one latest queued occurrence
  exist per schedule.
- Pause fences uncommitted queued/claimed occurrences. Resume and edit schedule
  only the next future slot, skipping paused history. A one-shot passed during
  pause ends as skipped/expired, never as delivered. Deletion revokes future work;
  already committed inbox entries remain. Old occurrence UTC identities remain
  unique across schedule revisions.

Supported calculation timestamps are1970 through2100-01-01 UTC. The maintained
`chrono0.4.45`/`chrono-tz0.10.4` libraries use a compiled IANA database; its explicit
version is persisted with each schedule. This is not a claim that the bundled
database is independently latest. A database-version change recalculates only
unexecuted future cursors; historical receipts and queued past instants stay exact.

## Authority, publication and restart

An explicit owner mutation persists a background grant pinned to tenant, account,
auth_epoch and schedule revision; it never fabricates a login session. Account
eligibility and current revision/state are checked before enqueue, claim and final
publication. Logout leaves this separate grant active. Password/recovery/account
epoch changes fence it and require an explicit current-owner resume/edit. Account
suspension is checked independently of the pinned auth epoch.

Claim alone creates no delivery authority. The same SQLite transaction rechecks
the grant, inserts the immutable notification, marks the occurrence delivered and
stores its notification receipt. Repeating a committed lease returns the same
receipt. Pause/edit first prevents publication; publication first remains delivered.
The local effect has no external send boundary. A dead process can requeue or
coalesce an uncommitted claim (30second lease), while committed notifications never
repeat after ordinary restart. A failure after inbox insertion but before receipt
commit rolls back both.

The sealed-current restore activation transaction pauses grants and skips uncommitted
occurrences. Fresh restored-account enrollment does not resume them; the owner must
explicitly act. Delivered receipts/inbox are preserved. Historical restore,
independent latestness and external task recovery remain outside this contract.

## Inbox and quiet hours

The local inbox is account-owned, with immutable title/message/occurrence identity,
actual publication time, delayed flag and quiet-hour state. Read and dismiss are
idempotent owner mutations; dismissal never resurrects through a concurrent read.
Quiet hours mark local entries without discarding them or claiming an external
Push was suppressed. Overnight windows are supported; enabled equal start/end is
rejected. Read/dismiss/quiet settings remain separate from schedule authority.

Lists are bounded:64 current schedules,100 inbox entries per keyset page, and50
occurrences/ranges each per history request. Old receipts and revoked tombstones
are retained; automatic history pruning/GC is not implemented. The React page uses
the existing account RequestScope plus synchronous request ownership/generations,
preserves unsaved edit/CAS state through refresh and fences old-account responses.
Navigating there preserves the existing chat draft in the parent workspace state.

## API

Authenticated current-account routes are `/api/v1/schedules` (GET/POST),
`/schedules/preview` (POST), `/schedules/{id}` (PUT),
`/schedules/{id}/control` (POST pause/resume/revoke), and
`/schedules/{id}/history` (GET). Inbox routes are `/api/v1/notifications` (GET with
optional `before` cursor/`dismissed=true`), `/notifications/{id}/read` and
`/notifications/{id}/dismiss` (POST), and `/notifications/settings` (GET/PUT).
No route accepts a caller-chosen account, clock, model or dispatch capability.

Schema0017 introduced the local scheduler. The current integrated schema0022 also
includes account lifecycle, sealed-current restore and media. See the original
[local reminder evidence](reviews/2026-11-13-local-reminders.md) and current
[conversational reminder integration](reviews/2026-11-13-conversational-reminders.md).

## Direct conversational reminders

The authenticated current message may create/change/pause/resume/delete a fixed
personal reminder without an additional approval step. This is a deterministic,
bounded grammar at Store admission; accepted commands never enter a provider run.
Ordinary messages use the existing chat path and incur no classifier/model call.
The existing verified model-choice admission requirement still applies to this
chat entry point even when the accepted reminder itself invokes no model.

Examples: `remind me in 10 minutes to stretch`, `remind me tomorrow at 9AM to
stretch`, `remind me every Monday and Wednesday at 09:00 to stretch`,
`10분 뒤에 물 마시라고 알려줘`, `내일 오전 9시에 자료 확인하라고 알려줘`,
and `매주 월요일,수요일 오전 9시에 스트레칭 알려줘`. Elapsed seconds/minutes/hours/days
use server UTC; intervals repeat no faster than60seconds. Today/tomorrow, Korean
모레, ISO dates, explicit AM/PM or 24-hour HH:MM use the browser IANA timezone.
A bare9 or Korean9시 without AM/PM clarifies; local one-off gaps clarify and folds
choose the first UTC offset. Recurring gaps/folds follow the occurrence policy
above. Unsupported grammar asks for a complete time and message again; there is
no stateful natural-language clarification continuation or semantic classifier.
Outer translation, summary, quote and ordinary information requests do not supply
reminder authority. An explicit `remind me ... to <message>` shell saves its entire
message literally, including summarize/translate/quote words, negation and question
punctuation. A Korean `...라고 알려줘` or explicit 알림 marker likewise identifies
literal reminder content. There is no keyword blacklist over the saved message.
Only current authenticated user text is inspected; retrieval, mail, skill text,
historical messages and model output cannot invoke this entry point. Known
one-off, relative-time and recurring research/search/send tasks receive an honest
unsupported response rather than becoming fixed-text reminders. For example,
`내일 오전 9시에 뉴스 조사해서 알려줘` is unsupported, while
`내일 오전 9시에 뉴스 조사하라고 알려줘` is a literal reminder.

A saved receipt exposes an actual owned schedule ID/revision, action, state,
next local slot, timezone and `/app/schedules` link. Selecting that receipt allows
`change reminder to tomorrow at 9AM`, `change reminder message to New text`,
`pause reminder`, `resume reminder` or `delete reminder`; Korean 알림 controls
and `알림 내용을 새 문구로 바꿔줘` are also supported. Otherwise an exact current
owned title or a single bounded candidate for a generic control is required.
Controls consume a complete imperative: `pause <exact title> reminder` or
`<정확한 제목> 알림 삭제해줘`. Named targets compare the complete parsed title,
never a substring of the user message. `the reminder` is a generic selector;
a leading `the` in a named title remains part of that literal title. A selected
receipt resolves identity and revision, not action authority: conditions, deferred
requests, questions, alternatives and quoted controls cannot mutate a schedule.
They clarify; stale selected revisions fail rather than silently retarget. `알림 꺼줘`
pauses rather than deletes; `알림 다시 켜줘` resumes. These on/off imperatives also
accept an exact owned title or the currently selected receipt. After a committed
change, the selected receipt advances to its returned revision only if the user
has not selected another target in the meantime. Deletion clears that selection.
The receipt selection clears with thread/account lifetime.

The browser freezes timezone and selected target in the uncertain-send payload.
Reusing the same request UUID and exact payload returns the original committed
chat and receipt after lost ACK/restart. A changed payload conflicts. Schedule
mutation, receipt, final confirmation and chat completion commit in one SQLite
transaction; a failed confirmation rolls all of them back. A command committed
before cancellation/logout remains committed. Logout/suspension/recovery before
admission rejects the stale user grant. There is no deferred schedule-write lease
or fabricated session. Existing epoch/revision/quarantine checks continue to
fence later background publication. Mail, browser and scheduled model missions,
changes-only comparison, topic mute and Push remain separate work.

Migration0022 adds the versioned chat receipt sidecar atomically after
recovery19/20 and media21. Confirmation uses the saved outcome and local date/time;
internal versions and storage slots are not part of ordinary chat wording.
Independent review, combined migration/restore checks and platform evidence are
recorded separately from the initial standalone implementation below.

See [standalone conversational reminder evidence](reviews/2026-11-13-conversational-reminders.md).

See [SC1–SC3 narrow parser repair evidence](reviews/2026-11-13-conversational-reminder-repair.md).

## 이미지와 함께 쓰는 대화

이미지가 첨부되었거나 이전 이미지의 문맥을 이어받은 대화에서는 알림을 생성·수정·
삭제하지 않는다. 실행하지 않았다는 안내를 표시하고 새 대화나 예약 화면으로 연결한다.
알림과 이미지의 선택·시간대는 처음 전송할 때 고정되어 동일 요청 재시도에 쓰이며,
같은 요청 ID로 이미지나 시간대를 바꾸면 충돌로 처리한다. 이미지 없이 새 대화에서
명확하게 요청한 알림은 모델 호출 없이 저장한다.

봉인된 현재 스냅샷을 복원하면 대화의 저장 영수증은 과거 결과로 보존하지만 모든
알림은 정지 상태다. 새 계정 등록만으로 재개되지 않는다. 주간 알림은 선택한 ISO
요일과 IANA 시간대에 따라 다음 미래 회차부터 계산한다.
