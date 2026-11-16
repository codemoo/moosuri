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

The model context includes a trusted reference for the original user message:
its stored UTC timestamp, canonical timezone, local RFC3339 timestamp and local
date. Relative dates use this reference, including retries across midnight or DST
changes. Older clients without a timezone use an explicit UTC fallback; this does
not claim to know the owner's local timezone or grant any write authority. The
reference survives tool/media instruction changes and history trimming.

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

Read-only JSON GET requests, including canonical history snapshots, have one
30-second deadline across connecting and reading the body. A timeout leaves loading
and shows the existing retry/refresh error; it never resubmits a mutation.
Browser SSE connections bound connecting to 15 seconds and missing raw bytes to 45
seconds; the server sends comment heartbeats every 15 seconds even during quiet
conversations. Reconnects use the saved cursor and never repeat a model submission.
The six-failure limit counts consecutive unhealthy connections; validated event
progress or a delayed heartbeat resets it. Fresh sends keep their live stream and
loaded history when SSE confirms the request within one second; missing receipts
and explicit same-ID resubmission still reload the canonical page. See the
[synthetic responsiveness checks](reviews/2026-11-15-responsive-chat-stream.md).

Main-chat message admission, new-thread creation and run cancel/retry requests
also opt into a 30-second deadline covering HTTP headers and complete JSON body.
Expiration means the receipt is unknown: the client checks canonical history,
preserves the same immutable message request ID/payload for explicit resubmission,
and retains the next draft. A canonical SSE/history receipt releases a stalled
message HTTP transport immediately. New-thread creation carries a separate immutable
request UUID/title. Unknown creation refreshes inventory without automatic replay;
explicit recovery reuses the same creation request, then the same message request
if message retry was chosen. Button-only recovery does not send the message.
The bounded authenticated tab-draft lease retains both identities beside the next
draft across reload. Receipt completion migrates that state atomically, and a
late receipt preserves another thread/draft selected while waiting. If the admitted
target already has another draft or unresolved message, recovery retains both
slots and original identities until that target is finished. Schema71
keeps an owner-scoped creation receipt: matching original title/UUID resolves the
same thread, changed title conflicts and deleted threads cannot be recreated by
replay. Old clients that omit the key keep their existing behavior. Other
mutation/probe/upload timing is unchanged. See the [foreground recovery checks](reviews/2026-11-15-chat-mutation-recovery.md).

## 진행 중인 작업에 추가 지시

서버가 추가 지시를 지원한다고 표시한 진행 중 작업에서는 주 입력창이 **진행 중인 작업에 추가**와
**다음 메시지 준비** 모드를 제공한다. 두 초안은 별도로 유지되며 추가 지시가 새 대화 턴을 만들지 않는다.
지원 여부가 늦게 확인되어도 이미 쓰는 다음 메시지 초안을 다른 모드로 옮기지 않는다. 작업이 끝나고
추가 지시 초안·접수 확인 중 요청이 없으면 원래 다음 메시지 입력으로 돌아간다.

추가 지시는 UTF-8 2,000바이트의 글만 받는다. 선택한 첨부가 있으면 다음 메시지에 남기고 글만
보낸다는 선택을 표시하며, 첨부를 삭제하거나 몰래 제외하여 보내지 않는다. 음성 변환과 공유받기로
넣은 글·파일은 다음 메시지용 입력에 남는다. 추가 지시 접수와 다음 초안·업로드는 독립적이다.

실제 영수증에 따라 **접수됨 · 아직 반영 전**, **반영됨**, **미반영 종료**를 원래 대화 턴 또는 업무
카드에 표시한다. 접수는 반영 완료가 아니며 별도 모델 응답을 꾸며 표시하지 않는다. 연결 오류나
새로고침 뒤에는 같은 요청 UUID의 상태만 조회한다. 영수증이 없을 때 같은 글·작업·시도 번호로
직접 다시 접수할 수 있으며 자동 POST는 하지 않는다. 종료와 접수가 겹쳐 거절되면 추가 지시 글을
보존하고 새 작업으로 바꾸지 않는다. 다음 작업에 비어 있지 않은 이전 지시를 자동 적용하지 않는다.

같은 업무가 재개되면 아직 보내지 않은 추가 지시 초안도 그대로 이어서 보낼 수 있다.
사용자가 전송을 눌렀을 때만 새 요청으로 접수한다. 이전 요청의 접수 여부를 확인 중이면
그 요청의 식별자를 유지하며, 다른 업무로 초안을 자동 이동하지 않는다.

원래 일회성 업무가 별도 실행으로 이어진 경우 서버가 반환한 실제 실행 대상에 추가한다. 업무 카드도
명시적인 지원 표시가 있을 때만 이 경로를 연다. 원래 허용한 도구·계정·실행 한도·종료 시각과
메일 최종 발송 확인을 늘리는 기능이 아니다. 입력 복구는 기존의 정확한 계정·로그인 세션,
최대 16개 슬롯·128KiB·8시간의 평문 tab-session 범위를 공유한다.

[프런트엔드 검증](reviews/2026-11-16-live-instruction-ui.md)과
[재개 중 초안 검증](reviews/2026-11-16-continuation-integrity.md),
[실행/영수증 계약](../openspec/changes/live-run-instructions/proposal.md)을 참고한다.

인증 도구가 포함된 빌드에서도 일반 읽기 대화는 진행 중 지시를 지원한다. 새 대화의
기기 메일·인증 도구 목록은 원래 요청으로 정하고, 재개·재시도에서는 기존 목록을
유지한다. 메일 언급이나 저장소 경로를 나타낼 수 있는 `/`가 있으면 보수적으로 인증
경로를 남겨 추가 지시가 제공되지 않을 수 있다. 기존 메일·브라우저 연결 및 실제 기기
작업의 지원 범위는 그대로다. [검증](reviews/2026-11-16-continuation-integrity.md).
