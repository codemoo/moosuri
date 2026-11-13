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
모델·자료 속 지시나 일반 채팅은 이 권한을 만들지 않는다. 현재 agent Calendar 도구는 없다.

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

이 구현은 F37의 소유권 확인한 기본 캘린더 읽기·직접 양식 쓰기 범위다. 자연어/agent/미션의
Calendar 실행, 반복 일정 편집, 참석자 초대/알림 메일, 보조 캘린더 변경, 종일 일정 쓰기,
실계정/실기기 검증과 배포 완료를 포함하지 않는다.

일정은 기기 현지 시간으로 표시하며 입력 시간대를 함께 안내한다. 수정 버튼은 편집기 제목으로
이동한다. 저장 응답을 기다리는 동안 이어 쓴 초안은 유지하고, 확인된 새 ETag로 다음 수정을
이어 간다. [합성 검증과 제한](reviews/2026-11-13-personal-calendar.md)을 참고한다.
