# 웹 조사와 이메일 실행 계약

사용자의 2026-11-13 추가 지시: 실제 브라우저 조사와 이메일 연결·대행을 개발하며,
**메일 발송은 반드시 최종 내용을 보여주고 허락받은 뒤 수행한다.** 이 문서는 개발 계약이다.
구현·시험·실계정 연결 상태는 [status](status.md)에서 별도로 표시한다.

## 웹

HTTP 검색/읽기와 Chromium 브라우저 조작은 다른 기능이다. 실제 검색 공급자의 결과와
실제 페이지 내용을 받는 `web_search`/`web_read`부터 연결한다. 미설정 검색은 사용 불가로
표시하고 합성 검색 결과나 HTTP 텍스트 읽기를 브라우저 조작으로 부르지 않는다.

읽기는 로그인 쿠키·Authorization 없이 public HTTP(S)만 사용한다. redirect마다 DNS
전체 결과를 검사하고 승인된 주소로 연결해 loopback/private/link-local/metadata 접근을
막는다. proxy 환경변수·자동 redirect/retry를 끄고 응답 크기·시간을 제한한다. HTML의
script/style 등은 본문에서 제외하고 동적 페이지의 한계를 표시한다.

실제 브라우저는 Rust worker와 별도 Chromium을 사용한다. 사용자별 Linux 격리 환경,
전용 프로필·다운로드 영역·자원 제한과 네트워크 프록시를 운영 진입 조건으로 둔다.
초기 URL만 검사하고 subresource나 JS가 사설 주소로 접근하는 구조는 허용하지 않는다.
브라우저 namespace는 승인된 proxy 이외 직접 TCP/UDP·QUIC·대체 DNS 경로를 갖지 않는다.
Chromium 자체 sandbox를 켜고 host home·앱 DB·Docker socket을 노출하지 않는다.

첫 브라우저 범위는 공개 페이지 열기·관측·스크린샷·닫기다. 페이지 handle은 사용자,
worker/context/target/frame/navigation generation에 묶는다. “마지막 탭”이나 임의 CDP
명령을 도구로 제공하지 않는다. stale handle·재시작은 관측을 새로 요구하고 알려지지 않은
쓰기 작업을 재생하지 않는다. 일반 읽기마다 사용자 승인을 요구하지 않는다.

인증된 사이트의 클릭·입력·폼 제출은 외부 작업 boundary 뒤에서 확장한다. 임의 웹메일
버튼 클릭이나 JavaScript로 아래 메일 승인 규칙을 우회할 수 없어야 한다. 버튼의 표시
문구를 보고 발송 여부를 판단하는 정규식은 권한 경계가 아니다. 인증 인계는 동일한
개인 브라우저와 원래 작업을 보존하며 다른 사용자·개인 기본 브라우저에 붙지 않는다.

## Gmail 연결

공용 LLM 연결과 달리 Gmail OAuth grant는 개인 계정에 속한다. 기존 provider 저장의
암호화 패턴은 재사용할 수 있지만 admin이 타인의 connector를 선택하거나 읽게 하지 않는다.
이 토큰은 서버가 관리하는 connector credential이며, 사용자만 해독할 수 있는 별도 볼트와
동일한 보호를 제공한다고 말하지 않는다. 모델·도구 결과·로그에 토큰을 보내지 않는다.

Google Cloud의 Gmail API와 OAuth web client, 정확한 redirect URI 설정이 필요하다.
미설정은 연결 준비 필요로 표시한다. 초기 범위가 로컬 초안이면 `gmail.readonly`와
`gmail.send`만 요청하고 Gmail 초안함에 저장했다고 말하지 않는다. 실제 Gmail draft API를
제공할 때는 `gmail.compose` scope와 해당 동작을 명시한다. `users/me`만 사용한다.

로그인 cookie가 SameSite=Strict이므로 Google callback에서 일반 인증 cookie나
`X-Moosuri-Account`를 기대하지 않는다. same-origin start가 one-use state를 원래
session/account/auth epoch와 묶고, callback은 짧게 암호화 보관한 code의 opaque handle만
UI로 넘긴다. 로그인된 same-origin complete가 원래 세션을 검증한 뒤 code를 교환한다.
state/code/PKCE verifier를 일반 로그·URL history·대화에 남기지 않는다. refresh/disconnect
경쟁은 connection revision으로 검사하며 늦은 refresh가 철회한 연결을 되살리지 않는다.

## 보내기 전 확인

모델의 도구는 메일 검색·읽기·thread 확인·초안 준비까지다. 모델 호출 가능한 raw send나
approve 도구는 없다. 이메일을 보내 달라는 사용자 메시지 자체로 최종 허락을 대체하지 않는다.

1. 사용자에게 From, To, Cc, Bcc, 제목, 전체 본문, 첨부를 표시한다.
2. 안정적인 Message-ID와 실제로 보낼 MIME/첨부 bytes·digest를 확정한다.
3. 사용자가 **허락하고 보내기**를 누르면 그 revision/digest만 승인한다. 내용·수신인·첨부·
   연결 또는 예약 시각이 달라지면 새 미승인 상태다. API는 승인 요청에서 새 본문을 받지 않는다.
4. 별도 worker가 현재 권한·연결·만료·취소를 확인하고 dispatch commitment를 먼저 기록한 뒤
   동결한 bytes만 보낸다. 읽는 동안 바뀔 수 있는 Gmail draft ID만 승인 대상으로 삼지 않는다.
5. Gmail의 message/thread ID가 오면 **Gmail 발송 접수**로 기록한다. 상대 수신·열람 완료라는
   뜻은 아니다. 성공 영수증 없는 완료 문장을 만들지 않는다.

첨부를 아직 지원하지 않는 초기 구현은 입력을 명시적으로 거절하며 몰래 제외하지 않는다.
첨부 지원 시 사용자 소유 blob을 먼저 고정하고 digest·크기·이름·형식을 preview와 결합한다.
승인 후 URL이나 host 파일 경로에서 새로운 bytes를 가져오지 않는다.

## 결과 불명과 철회

`pending_approval → approved → dispatch_committed → sent / failed_definite / outcome_unknown`.
승인 전이나 dispatch 전에는 거절·취소·만료가 가능하다. 두 번 클릭하거나 여러 worker가
경쟁해도 commitment는 한 번이다. 전송 이후 취소나 연결 해제는 이미 보낸 메일을 되돌렸다는
뜻이 아니며 늦게 도착한 영수증은 원래 작업에 기록한다.

commitment 이후 프로세스 중단·응답 유실은 자동 재발송하지 않는다. 안정적인 RFC Message-ID로
같은 보낸편지함을 조회해 일치하는 영수증을 찾을 수 있지만 Google은 Message-ID를 전송
idempotency key로 보장하지 않는다. 결과 0개는 색인 지연일 수 있으므로 미발송 확정이 아니다.
별도 재발송은 불명을 보여주고 새 내용 확인·허락을 받아야 한다.

모델이 읽은 자료에서 만든 초안은 원래 run과 근거 의존성을 기록한다. 자료 삭제/기억 망각으로
근거가 철회되면 아직 발송하지 않은 파생 초안·승인도 폐기한다. 이미 dispatch한 외부 결과는
소급 취소했다고 주장하지 않는다. 대화/사용자 삭제와 계정 전환에서도 같은 기준을 적용한다.

## 필수 검증

- public 문서 읽기와 redirect/subresource/DNS rebinding/private-IP canary 차단을 분리해 검증.
- 실제 Chromium 화면·DOM·스크린샷은 Linux 격리 확인 뒤 브라우저 지원으로 표시.
- OAuth replay/만료/계정 전환/누락 scope/refresh 철회 경쟁, cookie 없는 callback.
- 미승인·거절·변경·만료된 메일은 send 요청 0회; 원문 prompt injection도 승인 불가.
- preview와 보내는 bytes 일치, Bcc/Unicode/CRLF/첨부 검증, 중복 클릭/worker 1회 commitment.
- 중단·응답 유실·조회 0건/다중건에서 자동 재발송 0회, 늦은 영수증 보존.
- 두 사용자/admin의 읽기·초안·승인·첨부·세션 분리, 삭제 후 파생 payload 철회.

## 공식 근거

- [Gmail scope](https://developers.google.com/workspace/gmail/api/auth/scopes), [web OAuth](https://developers.google.com/identity/protocols/oauth2/web-server), [token 수명](https://developers.google.com/identity/protocols/oauth2).
- [Gmail send](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send), [검색](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/list), [draft 수명](https://developers.google.com/workspace/gmail/api/guides/drafts), [thread](https://developers.google.com/workspace/gmail/api/guides/threads).
- [Chromium proxy](https://chromium.googlesource.com/chromium/src/+/HEAD/net/docs/proxy.md), [Linux sandbox](https://chromium.googlesource.com/chromium/src.git/+/refs/heads/main/sandbox/linux), [CDP Target](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Target.pdl).
- [Brave search API](https://api-dashboard.search.brave.com/app/documentation/web-search), [reqwest transport](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html), [Agent Skills](https://agentskills.io/specification).
