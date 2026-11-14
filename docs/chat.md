# Durable text chat

The web app can send plain-text questions through a verified shared connection.
Choose a model in `/settings/model`; an installation default is also supported.
A new thread is created on the first message; its title uses that question’s opening text. Each account sees its own history,
model choice and usage. The administrator usage page shows counters, never text.

## Conversation behavior

- Submitted text is saved before dispatch. The browser retains a request UUID if
  the acknowledgement is lost, so sending that request again cannot duplicate it.
- Streaming is independent of the browser connection. Closing a tab does not stop
  the model task. Returning loads saved output and resumes committed events.
- Cancel stops accepting more visible output. Retry starts another attempt on the
  same question and preserves the composer draft. It keeps the original verified
  provider revision, model and effort; it never silently selects a fallback.
- Only the latest turn can be retried while conversation branching is unavailable.
  A cancelled/failed attempt can also be followed by a new question.
- Server restart marks interrupted dispatched work as `outcome_unknown`; it never
  automatically calls the model again. The interface offers an explicit retry.
- Deleting a conversation removes its text and event history and fences late model
  results. A minimal usage receipt remains without prompt or answer content.

Failed, incomplete, refused, cancelled and unknown outcomes remain distinct. Partial
answers remain readable with their status and are not reused as successful history.
Reported model identity must exactly match the selected ID. Unknown alias mappings
are not guessed. Commentary/answer phase is preserved when supplied by the provider.

## Bounds

One active run per thread and one dispatched call per account; at most two calls
run globally. Admission allows eight queued turns per account and64 globally.
A question is limited to8,192 UTF-8 bytes, with a64 KiB JSON envelope to accommodate
escaping. A call requests at most4,096 output tokens. A run has a120-second cumulative deadline,
at most6 model dispatches and12 local tools across retries, and at most256 KiB output.
Each step also bounds output items. Explicit attempts are capped at32; they do not reset
the smaller dispatch/tool/time budgets.
These are engineering limits, not monetary budgets or price estimates.

Context contains the latest whole successful turns that fit the message/byte caps.
It stops at an oversized turn, without bringing back even older context past that
gap. An omission note is explicit. Failed partial text is excluded. These are chat
history rules. Tool-verified models can additionally search/read the current account's
saved sources and memories; see [knowledge agent](knowledge-agent.md).

## API and storage

Authenticated requests assert the expected account in `X-Moosuri-Account`.
Session authority supplies the tenant; caller-provided identifiers cannot change it.

| Endpoint | Operation |
| --- | --- |
| `GET /api/v1/threads/{id}/messages?before=N` |20 chronological turns and a same-snapshot event cursor |
| `POST /api/v1/threads/{id}/messages` | `{request_id,text}` → durable turn |
| `GET /api/v1/threads/{id}/events?after=N` | Authenticated fetch SSE |
| `POST /api/v1/runs/{id}/cancel` | `{expected_attempt}` |
| `POST /api/v1/runs/{id}/retry` | `{expected_attempt}` |
| `GET /api/v1/usage` | Current account's observed counters |
| `GET /api/v1/admin/usage` | Account counters for administrators |

SSE `change` events use the committed sequence as their ID. `reset` requests a
fresh snapshot when the cursor is too old or ahead of the log; `closed` contains a
fixed error code. Each thread retains512 events. Streams use bounded queues, four
per account/64 globally, and revalidate authority before delivery. Already-sent
network bytes cannot be recalled; the UI rejects stale attempts and terminal deltas.
Knowledge withdrawal purges affected derived answers and advances the reset floor.
Every full browser snapshot replaces cached pages, including after a missed offline
reset, while retaining composer drafts and fencing late action/older-page responses.

SQLite migration3 adds turn/run/output/event and receipt tables; migration5 adds
per-step execution, private continuations, tool slots and knowledge dependencies.
Each step transactionally records dispatch intent before network IO. Opaque leases
bind tenant, attempt, step, nonce and authority epoch. All text writes repeat current lease checks. Supervisor exit
settlement cannot replay a model call. Process shutdown stops new calls and bounds
HTTP draining to3 seconds while the runtime drains within5 seconds.

Usage snapshots replace cumulative values rather than adding duplicates. Missing
provider counters remain unknown. Admin/user screens label totals as reported usage;
cancellation does not claim a refund. Cost accounting and configurable budgets remain
future work. The current runtime adds three bounded private knowledge read tools;
manual source ingestion and explicit memory management are available. Conversational
memory writes, automatic extraction, external tools, media and subscriptions remain pending.

See [verification](reviews/2026-11-12-durable-text-chat.md) for actual acceptance and
[status](status.md) for the remaining assistant implementation.

## Drafts after page refresh

After this server confirms the same login session, the current tab can recover its unsent composer,
selected existing thread and uncertain original request metadata. Recovery waits for the authenticated
thread list; deleted or missing threads lose their slots. It never sends automatically. An uncertain
request first checks saved history with GET. The existing **same request resubmit** button keeps its
original request UUID, immutable attachment IDs/revisions and target/timezone context. A saved failed
run is shown with its status; it does not replace the next draft in the composer.

Recovery uses one plaintext `sessionStorage` key, with at most16slots,128KiB total,8192 UTF-8 bytes
per text, and an8-hour lifetime since the last save. Over-limit text/slots may remain in the current
UI but are not guaranteed recovery. Corrupt/full/disabled storage falls back to in-memory chat.
It stores no credentials, API result bodies, source/artifact bodies, media bytes or upload handles.
It does not use localStorage, the Cache API or the service worker. Older servers without the optional
public session draft scope retain only the existing in-memory behavior.

Logout/authentication transitions and password changes clear the owned key immediately, even when
the logout network response is lost. A new login for the same account has a different public scope
and cannot load the prior session's drafts. Passive session revalidation also fences delayed UI
responses when that scope changes. Network failures do not establish authentication or authorize a
stored request. Automatic reconnect and **Try again** preserve inaccessible saved drafts while
rechecking the exact session and thread list, including when an earlier boot probe is still loading.
Late replies from abandoned boot probes cannot clear those drafts or replace the current session.
Neither retry automatically sends the pending request.

This is trusted-browser convenience, not vault encryption or durable backup. Same-origin scripts,
browser extensions and anyone with browser/OS access may read plaintext sessionStorage. Browsers can
copy a session into a duplicated tab or restore it after a crash/restart; tab-close erasure and crash
recovery are browser behavior, not guaranteed here. Recovery still requires the same current server
session. On shared devices, sign out and clear browser data as appropriate.

## Older conversation recall

New foreground tool profiles admit `conversation_search` and `conversation_read`.
Search defaults by instruction to the current thread; use `owned_conversations`
when other owned conversations are relevant to the current request. A literal query
scans the newest4096 eligible earlier successful turns and returns at most five
role-labelled snippet pairs with actual turn/thread IDs, revisions and timestamps.
Truncation is explicit. Turns exceeding10KiB whole text or14KiB encoded read content
are omitted. This is bounded lexical recall, so a miss does not prove global absence.

Read uses only an exact candidate returned to that run. Historical user assertions
and assistant answers remain distinct evidence, never new requests or approvals.
Every returned candidate is revocable, even if the model reads only another hit.
Source-thread deletion or upstream source/memory/mail/calendar/artifact/learned/media
withdrawal purges derived answers, private checkpoints and context descendants;
retry/reopen cannot recover purged output. Recall remains inside the existing
six-dispatch/twelve-tool/120-second run budgets. Stored old profiles gain no tools,
background missions exclude recall, and runs carrying recalled evidence cannot
publish it as a saved artifact.

Ordinary Korean requests such as “한국어 인사말 세 가지 부탁해” no longer add a
mission-classification model call solely because they contain “부탁”. Explicit
timing/delegation, selected-task controls and pending clarification keep their
existing routes. This removes one unnecessary round trip in that case; it is not
a measured general latency or live-model quality claim.

## 키보드와 대화 공간

사진·파일·음성·보관함을 입력창 위의 짧은 도구 줄에서 연다. 펼친 녹음·보관함은
입력 영역 안에서 스크롤할 수 있고, 작은 화면에서도 대화 영역을 남긴다. 입력창이
커지거나 미리보기가 늦게 나타나도 맨 아래를 보던 중이면 최신 메시지를 따라가고,
이전 내용을 읽던 중이면 보던 메시지 위치를 유지한다. 새 응답 버튼도 실제 입력창
높이 위에 놓인다. 긴 사이드 메뉴의 아래 계정 설정은 메뉴를 스크롤해 접근한다.

지원 브라우저에서는 키보드로 줄어든 visual viewport에 화면 높이를 맞춘다.
확대/축소는 제한하지 않으며, 확대 중에는 앱의 원래 배치를 유지한다. Chrome의
합성 화면 크기·비동기 높이 변화 검증과 실제 iOS/Android 키보드·접근성 수락은 별도다.
