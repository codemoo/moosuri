# 내 Google Calendar

사이드바의 **내 Calendar**에서 개인 Google 계정을 별도로 연결한다. 설치 관리자는 기존
Google 웹 OAuth 앱 하나와 정확한 callback URI를 준비하고, 같은 프로젝트에서 Google Calendar
API를 사용 설정한다. 사용자는 Calendar 연결에서 직접 동의한다. Gmail 연결·토큰을 재사용하거나
Gmail 권한을 자동 확장하지 않는다. 공유 모델 연결과도 별개다. 관리자도 다른 사용자의 달력이나
credential을 조회하지 못한다.

현재 요청 scope는 `calendar.calendarlist.readonly`와 `calendar.events.owned`다. 권한·지속 연결용
refresh token이 없으면 연결 완료로 표시하지 않는다. Google의 테스트 모드·정책·API 사용 설정은
로컬 JSON 확인만으로 검증할 수 없으며, 실제 연결 과정의 오류를 그대로 안내한다. 토큰은
서버가 관리하는 암호화 credential이며 사용자만 해독하는 별도 볼트의 보호를 주장하지 않는다.

연결하면 소유권이 확인된 **기본 캘린더**의 일정을 읽는다. 다른 캘린더는 목록 정보만 보인다.
조회 범위는 명시적인 시작·종료, 최대31일이고 한 페이지 최대100개다. 다음 페이지가 있으면
일부 결과임을 표시한다. 목록에 없는 일정이나 일부만 읽힌 자료를 삭제 근거로 사용하지 않는다.

직접 양식에서 참석자 없는 일반 시간 일정을 만들고, 수정·삭제할 수 있다. 기본 캘린더의
`primary=true`, `accessRole=owner`, 일정 organizer의 self/계정 ID를 검증한다. 참석자·반복/인스턴스·
종일·특수 eventType이 있거나 소유권을 확인하지 못한 일정은 읽기만 제공한다. 수정·삭제는
화면에서 읽은 ETag와 서버의 최신 ETag, Google `If-Match`가 모두 맞아야 한다. 충돌하면 현재
일정을 다시 불러와 확인한다. 임의 달력 ID나 다른 사용자의 connection으로 변경할 수 없다.

양식의 저장·삭제가 해당 요청의 명시적 권한이다. 추가 승인 창은 없다. 참석자 초대나 이메일
알림을 요청하지 않으며 `sendUpdates=none`, `sendNotifications=false`를 사용한다. 만들거나
수정하는 일정은 알림을 끈다. 캘린더 자체에 이메일 notification 설정이 있으면 보수적으로
읽기만 제공한다. Google에서 이를 끈 뒤 다시 불러와야 변경할 수 있다. 알 수 없는 외부 장치·
서드파티 알림까지 앱이 통제하거나 되돌린다는 보장은 아니다.

저장은 UUID와 정확한 payload digest로 접수한 뒤, 세션·인증 세대·연결 세대/revision·Google 앱
pin을 검사하고 SQLite에 dispatch를 먼저 확정한다. 동일 UUID의 동일 요청 재시도는 저장된
기록을 돌려주며 새 Google 요청을 보내지 않는다. 내용이 다르면 충돌한다. create의 provider
ID는 요청 UUID에서 고정하고, create/update에는 private operation/digest marker를 기록한다.
모델·자료 속 지시는 이 권한을 만들지 않는다. 현재 사용자 대화의 명확한 단일 요청만 아래 별도 admission을 거친다.

응답 유실·불확실한 provider 오류·프로세스 재시작은 **결과 불명**으로 남는다. 자동 재전송하지
않는다. 정상 실행의 HTTP 한도를 넘겨90초 이상 dispatch 영수증이 없으면 상태 새로고침에서
조회 가능한 결과 불명으로 전환하므로 같은 프로세스의 끊긴 HTTP 요청도 무한 대기하지 않는다. **저장 상태 확인**은 같은 event ID만 읽으며 요청 marker/digest와 원하는 필드가 모두
일치할 때만 저장을 확인한다. 변경되거나 찾을 수 없는 create/update는 결과 불명을 유지한다.
DELETE의404/410 조회는 ‘현재 없음’만 확인하며 이 요청이 삭제를 일으켰다고 단정하지 않는다.
직접 받은 성공 응답은 event/ETag 또는 DELETE204 영수증으로 남긴다. 복원 후에는 미완료 작업을
격리하고 credential/OAuth 상태를 폐기하여 이전 쓰기를 재생하지 않는다.

로그아웃·계정 세대 변경·연결 해제·credential/app 변경은 새 작업과 늦은 비공개 결과를 막는다.
이미 dispatch된 작업은 실제로 적용됐을 수 있어 최소 영수증/불확실성을 보존한다. 연결 해제는
기존 destructive journal에 남기며 토큰과 private 작업 payload/result를 제거한다. 늦은 refresh나 이미 시작한 OAuth 완료가
연결을 되살리지 못한다. 해제는 같은 소유자의 대기·교환 중 OAuth도 함께 폐기하며,
다시 연결하려면 해제 후 새 연결을 시작한다. 명시적 조회는 현재 세션의 읽기 권한과 최신 credential revision을
사용하지만 원래 쓰기 payload와 UUID는 바꾸지 않는다.

API: `/api/v1/calendar/connections`, `/oauth/start`, `/oauth/complete`, `/calendars`, `/events`,
`/actions`, `/actions/{id}/lookup`이며 이 경로들은 모두 `/api/v1/calendar` 아래다. Google callback은
기존 `/api/v1/mail/oauth/callback`을 함께 사용한다. Calendar의 구별 가능한 state prefix는 라우팅만
담당한다. 실제 권한은 hashed one-use state, 암호화 code/PKCE, 원래 세션·계정·epoch·app pin이다.
code는 callback에서 교환하지 않고 로그인된 same-origin completion으로 마친다.

이 구현은 F37의 소유권 확인한 기본 캘린더 읽기·직접 양식 및 아래 제한된 대화 쓰기 범위다. 미션의
Calendar 실행, 반복 일정 편집, 참석자 초대/알림 메일, 보조 캘린더 변경, 종일 일정 쓰기,
실계정/실기기 검증과 배포 완료를 포함하지 않는다.

조회·변경·불명 결과 확인은 공통 `CalendarService`를 통해 실행한다. HTTP 양식은 인증된
사용자 문맥을 전달하며, 같은 요청의 중복 전송 방지와 실제 영수증 처리를 한곳에서 유지한다.
대화와 양식은 같은 실행부를 사용하며 대화 권한은 별도 schema30 profile/plan으로 제한한다.
[공통 실행부 검증](reviews/2026-11-13-calendar-service.md)을 참고한다.

일정은 기기 현지 시간으로 표시하며 입력 시간대를 함께 안내한다. 수정 버튼은 편집기 제목으로
이동한다. 저장 응답을 기다리는 동안 이어 쓴 초안은 유지하고, 확인된 새 ETag로 다음 수정을
이어 간다. [합성 검증과 제한](reviews/2026-11-13-personal-calendar.md)을 참고한다.


## 제한된 대화 일정 작업

새 일반 대화에는 연결 시점의 계정/epoch/app pin과 서버 기준 시각·브라우저 IANA 시간대를
별도 profile로 고정한다. 이전 대화·미디어·미션의 저장된 권한을 확대하지 않는다.
`calendar_list_events`는 명시적인 RFC3339 최대31일 범위를 실제 provider 페이지당5개씩
읽으며, 이어 읽기는 같은 범위에서 실제 받은 next-page token만 쓴다. `calendar_read_event`는
같은 실행에서 목록이 발급한 opaque observation handle만 읽는다. 일부 목록은 부재나 유일한
대상을 증명하지 않는다. 큰 필드는 임의로 잘라 쓰지 않고 더 좁은 읽기를 요구한다.

현재 사용자가 한 개의 일반 시간 일정을 **지금** 만들기·수정·삭제하도록 명확히 요청하면
기존 업무 해석과 같은 한 번의 격리된 현재 요청 해석에서 Calendar domain을 선택한다.
고정 알림은 먼저 처리하며 평범한 비후보 대화에는 새 분류 호출이 없다. 예를 들어
`2026-12-01 10:00부터 11:00까지 팀 회의 등록해 줘`처럼 명확한 날짜·시작·종료(또는 기간)가
필요하다. 종료·기간을 한 시간으로 추정하지 않는다. 시작을 옮길 때도 새 종료/기간을 명시한다.
수정은 같은 날짜 안의 제목·시작/종료/기간 변경이며 다른 날짜로 이동은 추가 정보가 필요한 후속 범위다.
수정·삭제는 현재 요청의 정확한 기존 제목과 날짜, 완전한 작은 목록의 유일한 대상 및 새 ETag가
맞아야 한다. 중복 제목·다음 페이지·시간대/DST 불명확성은 추가 정보를 요청한다.

일정 시작 시각이 미래인 create와 미래에 명령을 실행하는 예약은 다르다. 예약 실행·반복·종일·
참석자·보조 캘린더 쓰기·여러 변경·이전 대화의 ‘그 일정’·메일/웹에서 새 값을 가져오는 변경은
이 범위에서 지원하지 않는다. 자료/모델은 봉인한 operation/target/field를 바꿀 수 없다.
일반 모델에게 쓰기 payload tool을 제공하지 않으며, Store가 봉인한 요청으로만 변경을 만든다.

한 writer transaction에서 원래 로그인 세션/epoch, 실행 lease, 봉인한 값, 연결/app 및 ETag를
검사하고 동일 UUID의 action과 대화 link를 함께 확정한다. 모델의 마지막 문장이 실패해도
실제 저장 상태의 영수증이 대화에 표시된다. 결과 불명은 재전송하지 않고 `/settings/calendar`의
기존 결과 조회나 같은 대화의 `calendar_action_status`로 읽기만 한다. 삭제404는 현재 부재이며
인과적 성공을 주장하지 않는다. 메일은 계속 매번 최종 미리보기의 사람 승인이 필요하다.

연결 해제·재연결·reconnect-required·앱 교체는 직접 및 이후 대화로 상속한 Calendar 근거,
출력·checkpoint·plan·observation을 철회하고 최소 action 영수증만 보존한다. 정상 token refresh는
역사를 삭제하지 않는다. 로그아웃·계정 인증 철회는 완료된 저장 대화는 보존하면서 원래 세션의
미적용 요청/진행 중 응답을 막는다. 복원된 profile/plan/observation은 실행 권한을 얻지 못한다.
Calendar 근거를 지속 문서로 저장하는 경로는 provenance 연결 전까지 차단한다. 직접 현재 문구의
literal 기억 명령은 기존 격리·정확 인용 계약을 유지한다.

합성 protocol/IO/SQLite/UI 검사는 연결과 실패 경계를 검증한다. 실제 Google 쓰기·동의/계정
정책·실기기와 넓은 한국어/영어 모델 이해도·지연 품질 검증은 별도이며 완료로 주장하지 않는다.


현재 대화 일정 작업을 취소하면 원래 요청의 Calendar 쓰기와 private 결과를 영구 철회한다.
원래 UUID·nonce·실제 상태·최소 영수증은 보존하지만 payload/event·봉인한 계획·관측·출력은
같은 취소 transaction에서 제거한다. 취소 뒤 Retry는 이전 요청을 다시 분류하거나 쓰기를
되살리지 않는다. 이미 나간 작업의 늦은 성공도 최소 영수증만 기록하며, 새 명시적 사용자
메시지는 별도 요청이다. 이 취소 철회는 로그아웃 시 완료된 저장 대화를 보존하는 정책과 별개다.


## 개인 ChatGPT foreground 연결

정확한 개인 계정/연결 revision/model/effort의 text와 tools 확인을 마친 모델도 위의 현재 요청 Calendar 작업을 사용한다. 별도 write probe나 승인 창은 추가하지 않는다. 현재 메시지만 분류하는 plan namespace 호출이 Calendar domain 또는 모든 추가 값이 null인 no_command만 제안할 수 있다. 개인 mission 생성·예약·제어는 Store의 결정 저장·preflight·실행 경계에서 거절하며, Calendar 작업을 미래에 실행하는 위임으로 바꾸지 않는다.

분류 전에는 P4와 같은 RunLease/token refresh CAS 및 plan preflight를 사용한다. 이미 봉인한 Calendar 요청은 새 모델 client나 분류보다 먼저 같은 UUID로 이어 간다. 실제 작업 영수증이 있으면 새 Google 쓰기나 마지막 모델 문장으로 재확인하지 않는다. 불명 결과 조회는 원래 ID만 읽으며 자동 재전송하지 않는다. 현재 요청 취소·로그아웃·연결/app 철회는 기존 private 결과 철회와 최소 영수증 계약을 유지한다. 개인 연결을 사용할 때도 참석자·반복·종일·보조 캘린더·background·첨부의 범위를 확대하지 않는다.

기존 개인 실행에 Calendar 해석 envelope를 소급 추가하지 않는다. 새 개인 turn에는 현재 Calendar profile을 고정하고 제한된 해석 경로를 접수한다. 연결 pin이 없으면 기존 typed receipt가 연결 필요 상태와 `/settings/calendar` 안내를 표시하며 쓰기를 하지 않는다. 이후 연결해도 이전 pin이 비어 있던 요청을 자동 실행하지 않고 새 요청을 받아야 한다. 계정이나 provider 이름으로 Calendar 권한을 추정하지 않는다. 합성 signed OAuth/plan classifier와 counted typed Calendar IO 검증은 실제 ChatGPT/Google 계정 동의·정책·모델 의미 정확도 검증을 대신하지 않는다.


개인 ChatGPT 연결을 해제하면 그 정확한 연결 revision에 봉인한 미완료 Calendar 명령과
진행 중 효과의 private payload/event·계획·관측·출력을 같은 transaction에서 철회한다.
이미 Google에 나간 작업은 적용됐을 수 있으므로 원래 UUID와 최소 영수증/불명 상태는
보존하지만, 늦은 응답이나 같은 개인 계정의 재연결·Retry가 private 결과나 이전 쓰기를
되살리지 않는다. 이미 완료된 Calendar 대화의 문장·영수증·이벤트는 모델 연결 해제만으로
삭제하지 않으며, Calendar 자체 철회와 기존 계정·세션 정책은 별도로 적용한다.
