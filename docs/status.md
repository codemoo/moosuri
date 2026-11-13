# 현재 상태

2026-11-13. 실제 구현·검증과 다음 작업은 이 문서가 정본이다. 기능 계약은 [features](features.md), 상세 단계는 [plan](plan.md)을 따른다.

공개 저장소는 [codemoo/moosuri](https://github.com/codemoo/moosuri)다. **계정·모델·저장되는 대화·개인 자료/기억·업무 스킬에 공개 HTTP·격리 Chromium 작업 공간과 개인 Gmail을 연결한 개발 프리뷰다. 메일은 최종 내용을 사용자가 승인한 뒤에만 발송한다. 브라우저 화면 보기·직접 로그인/조작·같은 화면에서 대화 이어가기를 지원한다. 자료 모음별 자동 기억 추출을 지원한다. 고정 로컬 알림 예약·소식함과 계정 관리, 이미지 첨부·실제 미리보기·보관함도 지원한다. 한글·영문으로 제한된 고정 알림 요청과 주간 반복을 지원하며, 예약된 모델 업무·인증된 사이트의 자동 조작은 개발 중이다.** GitHub Actions/CI는 사용자 지시로 꺼져 있다. 작업 단위마다 로컬 검증 후 커밋·푸시한다.

## 구현된 첫 단위

| 영역 | 실제 동작 |
| --- | --- |
| 실행 | Rust 1.89 CLI/API, React 정적 파일을 포함한 서버, SQLite 단일 writer, Linux 실행 |
| 초기 설치 | 숨김 비밀번호 입력 또는 owner-only 파일, 최초 admin 한 명, 비워 두면 일회성 임시 비밀번호 |
| 회원 | admin의 이름·이메일 기반 로컬 초대, 24시간 임시 비밀번호, 최초 변경 전 개인 API 차단 |
| 세션 | HttpOnly cookie, 서버의 30일 유휴 만료·활동 갱신, 비밀번호 변경 시 이전 세션 철회 |
| 사용자 격리 | 세션에서 정한 tenant, 매 작업 재검증, admin도 타인의 대화 접근 불가 |
| 웹 | 로그인·비밀번호 변경·초대·회원 목록, 빈 대화 생성·목록·삭제, 모바일 별도 스크롤 |
| 탭 전환 | 이전 계정 요청 중단/결과 폐기, expected-account 검사, 탭 간 갱신, 늦은 로그아웃의 쿠키 덮어쓰기 방지 |
| 저장 | private 파일·경로 검증, 프로세스 writer lock, bounded queue/KDF, auth epoch journal의 중단·잘림 탐지 |

설치·빌드·프록시·로컬 검증 명령은 [development](development.md)에 있다. 초대 이메일은 아직 발송하지 않는다. 아래 네 번째 단위에서 이 계정 기반에 실제 텍스트 대화를 연결했다.

## 검증한 범위

- Rust 1.89에서 fmt·clippy(`-D warnings`)와 **17개 테스트** 통과. 파일 DB·별도 프로세스 writer 경쟁·임시 비밀번호 교환 경쟁·tenant 경계·journal 중단을 검사한다.
- macOS와 Linux x86_64의 격리 Docker 환경에서 같은 17개 테스트 통과. 실제 CLI 초기화→내장 웹→HTTP 로그인→재시작→세션 유지→대화 삭제까지 포함한다. 운영 서비스 배포는 하지 않았다.
- React의 **18개 행동 테스트**, TypeScript 검사와 production build 통과. Rust에서 생성한 웹 DTO 일치 확인.
- Chromium 실제 서버 여정과 두 탭 회귀 검증의 범위는 [계정 기반 검증 기록](reviews/2026-11-12-account-foundation.md)에 기록한다. 모바일 viewport 확인은 실제 iOS/Android 검증과 구별한다.
- 새 소스의 인증·저장·브라우저 경계를 독립 검토했다. proxy/NAT 로그인 제한 공유, 잘못된 디렉터리 권한 변경, 다른 탭 계정 혼동, 늦은 로그아웃 응답 문제를 수정하고 재검토했다.
- SQLite advisory에 따라 rusqlite 0.41.1/libsqlite3-sys 0.39.1로 갱신하고 DB open 전에 SQLite 3.54 이상을 확인한다. 첫 계정 단위 Cargo.lock의 registry 패키지 153개 OSV 조회에서 알려진 취약점 응답 없음. 이는 취약점 부재 보증이 아니다.

기존 테스트는 복사하지 않았다. 새 테스트도 전체 제품 평가 성적으로 사용하지 않는다. source523·entrypoint108과 과거 검토 기록은 유지한다. 현재 계획 추적은 R29/F70·70개 planned 평가 묶음으로 확장했다.

## 두 번째 단위: 모델 전송 기반

`moosuri-connectors`에 Responses·Anthropic Messages·OpenRouter·compatible Chat의 text protocol/HTTP adapter를 구현했다. 종료/사용량/phase를 구별하며 최종 본문을 스트림과 대조한다. 취소·부분 오류·잘림·DNS/credential 경계·timeout·backpressure를 합성 자료와 실제 loopback HTTP로 검증했다.

- 새 connector43개(프로토콜32·HTTP11), 기존 계정17개를 합쳐 **Rust60개 테스트가 macOS와 Linux x86_64에서 통과**했다. fmt·workspace/all-target clippy도 통과했다.
- 전송/프로토콜을 독립 검토하고 메타데이터 주소·수신 중단·오류 전 부분 출력·최종 본문 대조 문제를 수정해 재검토했다.
- 현재 lock의 registry224개 OSV 조회에서 알려진 취약점 응답 없음. CI는 계속 비활성이다.
- 이 두 번째 단위 자체는 웹 composer/provider 설정·실계정 모델·구독 인증을 연결하지 않았다. 모델 성능이나 미션 수행 성적으로 표현하지 않는다.

[상세 계약](model-transport.md)과 [검증 기록](reviews/2026-11-12-model-transport.md)을 따른다.

## 세 번째 단위: 공용 모델 설정

관리자 연결 화면과 사용자별 model/effort 선택을 실제 Rust API·SQLite에 연결했다.
OpenAI·Anthropic·OpenRouter·로컬 compatible 연결을 등록하고, 모델 목록 조회와
짧은 합성 응답 검증 후 기본값/개인 선택으로 저장한다. 목록 자체를 기능 검증으로
취급하지 않으며, 완료·본문·응답 모델 ID까지 확인한다.

- XChaCha20-Poly1305와 별도 private 설치 키로 공용 API credential을 저장한다.
  반환 DTO에는 key 대신 configured 상태만 있다. 키 파일 유실/교체·암호문 변조는
  fail-closed이며 설정 변경 시 이전 암호문을 논리 삭제한다. 사용자 키 볼트와 별개다.
- 설정 revision·기본값·개인 선택을 고정하고 stale edit/probe/선택을 거절한다.
  해제·수정한 연결을 다른 provider/default로 자동 대체하지 않는다.
- 새 provider 저장5개·HTTP3개·catalog2개를 포함해 **Rust70개 테스트가 macOS와
  Linux x86_64에서 통과**했다. fmt·workspace/all-target clippy도 통과했다.
- React **29개 테스트**, TypeScript 검사·production build·생성 DTO 일치 확인.
  Chromium에서 실제 Rust 서버의 no-key 로컬 연결→목록→검증→설치 기본값→
  개인 선택→새로고침을 확인했다. 실제 모바일 기기 시험은 아직 아니다.
- 저장/암호/권한/전송과 React 계정 경계를 독립 검토했다. 응답 모델 확인 누락과
  no-key 새 연결의 잘못된 Keep 요청을 수정하고 재검토했다.
- registry232개 dependency OSV 조회에서 알려진 advisory 응답 없음. 실서비스 키와
  유료 모델 호출은 사용하지 않았고 CI는 계속 비활성이다.

사용 방법은 [provider settings](provider-settings.md), 검증과 한계는
[검증 기록](reviews/2026-11-12-provider-settings.md)에 둔다. 구독 인증이나 전체 P0/P1/P2 완료로 표시하지 않는다. 실제 채팅 실행은 아래 네 번째 단위에서 연결했다.

## 네 번째 단위: 저장되는 텍스트 대화

검증된 모델 연결로 실제 텍스트 질문을 보내고, 스트리밍·기록·재접속·취소·재시도를
사용할 수 있다. 첫 질문으로 대화 이름을 붙이고 사용자별 문맥과 초안을 분리한다.

- 요청 UUID로 중복 접수를 합치고, 재시도는 같은 질문의 새 attempt로 이어 간다.
  브라우저를 닫아도 실행은 저장된다. 실행 중 서버 재시작은 결과 불명으로 남기고
  모델 호출을 자동 재생하지 않는다. 재시도 때문에 입력 초안을 바꾸지 않는다.
- 취소·삭제·세션/연결 철회가 늦은 출력을 막는다. 삭제 후 본문 없는 호출 영수증은
  사용량 관측을 위해 남는다. 관리자는 사용자별 카운터만 보고 개인 대화를 읽지 않는다.
- 대화 SSE는 저장된 cursor와 연결되고, 늦은 ACK/이전 attempt/phase 변경을 구별한다.
  긴 대화는 최신 성공한 완전한 turn의 연속 구간만 사용하며 생략을 명시한다.
- **Rust93개 테스트가 macOS와 Linux x86_64에서 통과**했다. 새 검증은 저장8개,
  runtime12개, HTTP3개이며 기존 CLI 시험에 느린 요청의 종료 검증도 추가했다.
- React48개 행동 테스트·타입 검사·production build를 통과했다. 실제 Chromium 검증과 상세 결과는 [검증 기록](reviews/2026-11-12-durable-text-chat.md)에 둔다.
  모바일 viewport 확인은 실제 모바일 기기 검증과 구별한다.
- Store·runtime·HTTP/SSE·React를 독립 검토했다. 끊긴 실행의 종료 상태 불일치,
  오래된 문맥의 잘못된 복원, 임시 writer 압력의 권한 철회 오판, 무한 HTTP drain,
  JSON escape 크기, 늦은 ACK의 본문 중복, phase 해제 처리를 수정·재검토했다.
- registry232개 dependency는 이전 검증과 동일하다. 실서비스 모델 키·운영 데이터는
  사용하지 않았고 CI는 계속 비활성이다. 사용량은 보고된 값이며 가격/지출 상한이 아니다.

사용법과 경계는 [text chat](chat.md)에 있다. 전체 비서·기억·미션 수행 완료나 실모델
품질/속도 평가로 표현하지 않는다.

## 다섯 번째 단위: 개인 자료와 명시적 기억

자료 모음에 텍스트를 저장하고 한글을 검색하며, 실제 원문 구간을 근거로 기억을
만들거나 직접 기록할 수 있다. 기억 수정·이력·망각, 자료 삭제, 모음 보관/삭제를
계정별 Rust API·SQLite·React 화면에 연결했다. 채팅 검색과 자동 추출은 다음 단위다.

- batch 요청 UUID·digest·cursor CAS로 응답 유실 재시도를 합치고, 자료·색인·영수증·
  cursor를 한 transaction으로 저장한다. 부분 입력에서 누락된 자료를 지우지 않는다.
- 근거의 원문 revision과 UTF-8 byte 구간을 검사한다. 원문 변경은 의존 기억을
  무효화하며, 삭제는 의존 원문/기억 이력을 제거한다. 독립적인 후속 기억은 보존한다.
- 시간 기준 검색은 시작된 최신 revision을 먼저 정한다. 만료·삭제 때문에 과거 사실이
  자동 부활하지 않는다. 망각은 원문 삭제와 구별되며 자동 재추출 방지는 후속 범위다.
- 파괴적 변경은 opaque journal을 fsync한 뒤 적용하고 startup replay한다. schema4
  설치에서 필요한 journal이 없거나 훼손되면 열지 않는다. 백업/복원 완료 주장은 아니다.
- **Rust111개 테스트가 macOS와 Linux x86_64에서 통과**했다. Store38·runtime12·
  connector45·HTTP15·CLI1이며 fmt·workspace/all-target clippy도 통과했다.
- React65개 행동 테스트, 타입 검사·production build·생성 DTO 일치를 확인했다.
  저장/삭제/시간 경계·HTTP와 React를 독립 검토하고 네 결함을 수정·재검토했다.
- 실제 Chromium 여정과 한계는 [검증 기록](reviews/2026-11-12-private-knowledge.md)에
  둔다. registry232개 dependency는 이전 검증과 동일하며 운영 자료·실모델 호출을
  사용하지 않았다. CI는 계속 비활성이다.

사용법과 삭제 범위는 [private knowledge](knowledge.md)에 있다. 어휘 검색·수동 관리
기반이며 의미 검색 품질이나 P3 전체 완료로 표시하지 않는다.

## 여섯 번째 단위: 검증된 모델 도구 호출 전송

네 가지 기존 protocol에 typed function request·완료 call·private continuation을
추가했다. 이 단위 자체는 library다. 아래 일곱 번째 단위에서 웹 채팅 runtime에 연결했다.

- 도구 지원은 명시적으로 켜야 하며 기본은 off다. text 검증·catalog를 도구 지원으로
  간주하지 않는다. 원래 사용자 권한·typed argument·작업 영수증은 다음 runtime의 책임이다.
- Responses reasoning/phase, Anthropic thinking/signature/redaction, OpenRouter
  reasoning details와 call/result ID를 보존한다. private capsule은 dialect/model/effort에
  고정하고 브라우저 DTO·일반 Debug 출력에 내보내지 않는다.
- preview에서는 실행 가능한 call이나 Finished를 내보내지 않는다. 완성된 terminal·
  identity·JSON 검증과 clean HTTP EOF 뒤에만 완료 call을 반환한다. 조각 경계에 따라
  후속 오류를 놓치던 결함을 독립 검토에서 발견해 수정·재검토했다.
- **Rust125개 테스트가 macOS와 Linux x86_64에서 통과**했다. Connector는59개이며,
  별도 HTTP 조각의 오류/중복 종료·held EOF 취소/timeout과 기존 text 동작을 검사한다.
  fmt·workspace/all-target clippy도 통과했다.
- 웹/DTO/DB와 Cargo/npm lock은 변경하지 않았다. 직전 React65개 검증을 유지하며
  새 웹 여정으로 표시하지 않는다. 실모델 호출이나 미션 성공률 평가는 아직 아니다.

상세 계약·공식 참고 자료는 [model tools](model-tools.md)에 있다. 다음 단계에서
호출별 저장·누적 예산·자료 의존성/철회와 실제 검색 도구를 연결한다. CI는 꺼져 있다.

## 일곱 번째 단위: 자료를 읽는 저장형 에이전트

도구 지원을 확인한 모델이 개인 자료·기억을 검색하고 정확한 revision의 원문을 읽은
뒤 답변을 이어 간다. 채팅에는 서버가 기록한 실제 참조를 표시하고, 원문/기억의
해당 버전을 열 수 있다. 본문 없는 출처 버튼이나 모델이 지어낸 ID는 표시하지 않는다.

- 관리자 도구 확인은 고정 함수 호출→새 challenge 결과의 정확한 반환을 두 번의
  합성 호출로 검사한다. 기존 text proof·catalog만으로 활성화하지 않으며 provider
  revision/model/effort와 현재 권한을 확인한다. 이전 run의 기능 설정은 바꾸지 않는다.
- SQLite migration5에 단계별 dispatch 영수증·private continuation·typed tool slot과
  결과·의존성을 추가했다. 완성된 모델 호출을 저장한 뒤 도구를 실행하며, 재시작이나
  결과 불명 호출을 자동 재생하지 않는다. 명시적 retry도6 model/12 tool/120초 예산을
  초기화하지 않는다. 모델별 단계의 관측 사용량은 삭제·취소 후에도 기록할 수 있다.
- 실제 반환된 근거와 성공한 이전 문맥의 의존성을 연결한다. 자료 수정/삭제·기억
  수정/망각은 관련 답변과 내부 capsule/slot을 철회한다. 질문과 사용량은 유지한다.
  진행 중인 자료 의존 run은 관련 없는 자료 변경에도 보수적으로 중단될 수 있다.
- SSE reset뿐 아니라 모든 전체 새로고침에서 예전 페이지를 교체한다. 오프라인 동안
  reset을 놓치거나 늦은 재시도 응답이 와도 삭제된 답변을 되살리지 않고 초안은 유지한다.
- macOS·Linux x86_64에서 **Rust154개**(Store57·runtime18·connector59·HTTP19·CLI1),
  fmt·all-target clippy를 통과했다. React70개, 타입/production build·생성 DTO 일치도
  확인했다. 실제 Chromium 여정과390×844 화면을 검증했으며 실제 모바일 기기 시험은 아니다.
- Store·runtime/HTTP·React를 독립 검토했다. 누락된 reset 후 과거 페이지 보존,
  예산 초과의 권한 오류 오표시, 큰 인자로 인한 불필요한 전체 실패를 수정·재검토했다.
  합성 runtime 회귀6개와 HTTP/SSE 회귀1개가 실제 모델 loopback과 file DB를 사용한다.

상세 동작·한계는 [knowledge agent](knowledge-agent.md), 실제 브라우저/플랫폼 수락은
[검증 기록](reviews/2026-11-12-knowledge-agent.md)에 둔다. 자동 기억 저장·외부 작업·
전체 미션 능력은 구현 완료로 표시하지 않는다. registry dependency는 이전232개와
동일하며 운영 데이터·실모델 키는 쓰지 않았다. CI는 계속 비활성이다.

## 여덟 번째 단위: 대화로 기억 저장·정정·망각

도구 지원을 확인한 모델에서 한 가지 사실을 “기억해 줘”, “수정해 줘”, “잊어 줘”로
관리한다. 명확한 요청에 승인 단계를 추가하지 않는다. 결과는 실제 변경 영수증의
고정 확인문이며 저장/정정은 실제 기억 revision을 연다. 이전 대화에서 파생한
답변은 철회하고 입력 초안은 유지한다.

- 현재 사용자 발화만 별도 모델에 보내고, 완성된 함수 결정·정확한 인용·크기·형태를
  검증한다. 추출 자료나 이전 답변은 기억 변경 권한이 아니다. 계획 중 본문은 숨기며
  실제 호출 사용량과 누적 실행 한도는 유지한다.
- 실행당 하나의 봉인된 명령과 결과 영수증을 저장한다. 재시도는 같은 의도를 재사용하고
  이미 자료를 읽은 실행은 다시 명령을 해석하지 않는다. 수정/삭제 대상은 현재 유효한
  버전으로 찾고 head와 effective revision을 따로 고정하며 변경/모호함은 쓰기 없이 확인한다.
- 망각은 독립 journal에 확정한 뒤 적용한다. 확정 전 준비 상태는 재시작 때 자동 실행하지
  않고, 확정 뒤에는 세션/기억 유효기간이 끝나도 같은 삭제를 복구한다. 대화가 삭제돼도
  복구용 식별자는 남지만 대화를 되살리지 않는다.
- macOS·Linux x86_64에서 **Rust191개**(Store74·agent37·connector59·HTTP20·CLI1),
  fmt·all-target clippy를 확인했다. React70개·타입 검사·production build를 통과했다.
  실제 계정 모델 평가는 일반 테스트와 별도의 opt-in으로 실행한다.
- Store·planner·runtime/HTTP를 독립 검토했다. 미래 head 때문에 현재 모호함을 놓치는
  삭제, 이미 읽은 실행의 재분류, 확정 삭제 뒤 만료된 기억의 복구 문제를 수정했다.
  실제 Chromium에서 저장→다른 대화의 회상→정정→망각·근거 철회와390×844 화면을 확인했다.

사용법과 제한은 [conversational memory](conversational-memory.md), 검증은
[수락 기록](reviews/2026-11-12-conversational-memory.md)에 둔다. 다른 언어·간접 표현·
정교한 시간 해석이나 의미상 모든 망각을 보장하지 않는다. CI는 계속 비활성이다.

## 실제 ChatGPT 계정 품질 확인

기존 ChatGPT 로그인을 공식 Codex app-server로 재사용해 **gpt-6.1-terra / high**를
호출했다. 개발용 합성 자료와 별도 SQLite만 사용했으며, 로그인 토큰을 제품이나
Linux로 복사하지 않았다. 일반 사용자를 위한 구독 로그인 기능의 출시와는 별개다.

- 마지막 명령 분류는 실모델 **36/36**, 모델을 부르지 않는 cue 경로는 **5/5**였다.
  반복한 공개 개발 corpus의 decision/action/clarification 검사이며, 추출한 대상의
  의미 정확도나 비서 전체 품질 점수가 아니다. 이전35/36 실패도 기록에 남겼다.
- 실제 Rust HTTP·저장소를 거친 대화 시나리오는 **7/10**이다. 저장·새 대화의 회상,
  부정/인용 처리·없는 대상 보존·짧은 계산·대화 이어가기를 통과했다. “홍차 취향”의
  수정 대상 찾기가 실패했고 뒤의 망각/재조회 시나리오도 연쇄 실패했다.
- 원문을 지정한 별도 수정→망각→재조회는 **3/3**이다. 같은 기억의 revision 변경,
  실제 삭제와 파생 답변/근거 철회를 확인했다. 앞선 실패 점수를 대체하지 않는다.
- 범주 질문의 회상을 위해 현재 유효한 기억을 제한된 페이지로 조회하도록 개선했다.
  실제 cursor를 모델에 전달하고 잘못된 cursor/query 조합은 수정 가능한 도구 오류로
  저장한다. 대상 수정/삭제는 유일한 어휘 일치를 우선 사용한다. 어휘 대상이 없으면
  현재 요청에서 이미 확정한 명령과 전체 제한 후보를 봉인한 뒤 별도 모델이 후보
  index만 선택한다. 모호한 어휘 일치나 후보64행/32KiB 초과는 명확화를 요청한다.
  세대·전체 후보·현재 유효 revision·head 변경은 저장 직전 다시 검사한다. 후보마다
  기억/근거 의존성을 등록해 망각·자료 철회 때 private 후보/체크포인트를 함께 지우고,
  재개·모델 호출 진입 전에도 전체 후보를 재검사한다.
- 분류 호출 중앙값은4.834초,10개 대화 시나리오의 완료 중앙값은5.846초였다.
  전자는 app-server 시작 시간을, 후자는 임시 bridge를 포함하므로 제품 스트리밍
  지연이나 TTFT 측정으로 해석하지 않는다.

새 후보 선택 경로는 합성 Store/대본 runtime 검증이며 실모델로 다시 평가하지 않았다.
후보 해석의 의미 정확도·주입 내성은 아직 검증해야 하며 기존7/10 점수를 개선했다고
보고하지 않는다. 합성 Store 기억 명령31개와 agent 기억 관련21개(실모델1개 ignored),
해당 두 crate의 all-target clippy·fmt·계획/OpenSpec 검사를 통과했다. 실행 구성·모든 점수·제한과 재실행 방법은
[실계정 평가 기록](reviews/2026-11-13-live-chatgpt-quality.md)에 둔다.

## 아홉 번째 단위: 자료 기반 업무 스킬

자연스러운 요청에서 읽을 절차를 고르는 자료 브리핑·회의 준비·근거 비교 스킬과
정확한 소수 계산을 기존 채팅 실행에 연결했다. Muse 중심으로 공개 업무 사례17개를
조사하고 제품 문서·홍보·시연·후기와 무수리의 파생 제안을 구별했다.

- 새 실행은 스킬 revision/digest와 도구 구성을 고정한다. 기존 실행은 기존 도구를
  유지하고, 재시도 때 내용을 바꾸거나 예산을 초기화하지 않는다. 스킬 읽기는 새 권한이 아니다.
- 합·차·곱은 bounded decimal 문자열과 checked 정수 연산으로 계산한다. 자료 삭제는
  그 자료에서 파생한 비교 답변과 계산 payload도 철회한다. 별도 일반 업무 분류기는 추가하지 않았다.
- Linux x86_64에서 Rust198개(Store77·agent41·connector59·HTTP20·CLI1), macOS에서
  관련 조합 검증과 fmt/all-target clippy를 통과했다. 기존 React70개 검증은 유지한다.
  스킬/실행/저장 경계의 독립 검토에서 중대한 결함은 없었다.
- 첫 공개 당시 실제 ChatGPT 도구 확인 문자열 반환이 실패해 업무5개는 미실행이었다.
  후속 검증에서 공개 challenge를 짧은 무작위 문자열로 바꾸고 기존의 정확한 검사
  전체를 유지한 채 정상 API proof를 통과했다. 첫 업무 시험4/5, 호출 지침 보완 후
  같은5개 결과5/5를 확인했다. 금액 계산 도구 사용 누락은 별도 지침과 재시험으로
  2/2를 확인했다. 작은 반복 개발 corpus이며 이전 실패나 전체 품질 점수를 대체하지 않는다.

[사용법](task-skills.md), [공개 사례와 출처](research/personal-agent-tasks-2026-11-13.md),
[검증 기록](reviews/2026-11-13-task-skills.md)에 상세 범위를 기록했다. 브라우저·Gmail은
이 단위에 포함되지 않으며 [별도 실행 계약](external-work.md)에 따라 개발 중이다.
메일은 최종 수신인·내용·첨부를 보여주고 명시적인 허락을 받은 뒤에만 발송한다.

## 열 번째 단위: 공개 웹 조사와 개인 Gmail

채팅의 `web_read`는 실제 공개 HTTP 페이지를 읽고, Brave 키를 설정하면 `web_search`도
사용한다. 페이지와 검색 snippet을 구별하며, 저장된 실제 관측 URL·제목·시각을 대화에
표시한다. HTTP 읽기는 JavaScript 브라우저라고 표현하지 않는다.

- 개인 Gmail OAuth, 검색·본문/thread 읽기, 무수리 로컬 초안과 편집·승인 화면,
  별도 발송 worker를 연결했다. 모델에게 approve/send 도구는 없다. “보내 줘”라는
  요청도 먼저 From/To/Cc/Bcc·제목·전체 본문·첨부 없음을 보여주고 최종 허락을 받는다.
- 승인은 동결한 MIME·thread·Message-ID의 정확한 revision/digest에 묶인다. 수정 중
  오래된 초안을 승인할 수 없고, 이중 클릭·재시작·응답 유실이 자동 재발송을 만들지 않는다.
  결과 불명은 불명으로 남기며 Gmail의 message/thread 영수증과 수신·열람을 구별한다.
- 웹/Gmail 네트워크 IO는 SQLite 밖에서 수행한다. 기존 누적 예산·현재 계정/연결·
  durable slot/nonce·철회 검사를 유지하고, 자료 삭제와 연결 해제는 파생 답변·미발송
  초안·열린 편집기의 폐기된 내용을 제거한다. 일반 조회에는 사용자 승인 단계를 추가하지 않았다.
- macOS/Linux x86_64 Rust workspace244개, React78개와 타입·production build·DTO 일치를 통과했다.
  독립 검토에서 드러난 MIME 일부 본문 표시, SSE 철회 경쟁, 편집 중 승인과 폐기된
  편집기 내용을 수정하고 재검토했다. HTTP 웹 통합의 추가 독립 검토도 완료했다.
  상세 검사 결과는 [수락 기록](reviews/2026-11-13-external-work.md)에 기록한다.
- 실제 Chromium에서 합성 초안 확인→수정→저장→로컬 승인과390×844 스크롤을 확인했다.
  Google 발송 설정은 꺼둔 시험이며 실계정 OAuth·유료 Brave 검색·실제 메일 발송은 미실행이다.

설정은 [Gmail](mail.md)과 [웹 조사](web-research.md)를 따른다. OAuth 토큰은 서버 관리
connector credential이며 사용자 키 볼트와 같은 보호를 주장하지 않는다. Gmail 첨부·HTML·
발신 별칭·라벨 변경·Gmail 초안함과 자동 보낸편지함 대조는 아직 지원하지 않는다.

## 열한 번째 단위: 실제 Chromium으로 공개 페이지 읽기

채팅의 `browser_read`를 별도 Linux Chromium capsule과 연결했다. 공개 URL 하나를
받아 실제 렌더링된 본문·제목·링크·조회시각을 반환하고, 대화에 HTTP와 구별한
브라우저 출처를 표시한다. 검증한 배포에서만 활성화하며 기존 실행의 권한은 늘리지 않는다.

- fresh capsule·공개 목적지 proxy·Chromium sandbox·namespace firewall·계정 readiness와
  정확한 image/network/proxy pin을 사용한다. 모델은 Docker·임의 CDP·credential·클릭·폼
  권한을 갖지 않는다. 일반 페이지 읽기에 사용자 승인 화면을 추가하지 않았다.
- 기존 durable 도구 슬롯·철회·누적 예산·취소를 유지한다. 자동 container 제거와 강제 제거의
  경합으로 정상 읽기가 실패하던 결함을 실제 Linux 시험에서 찾아 수정했다. 실패한 제거는
  같은 불변 ID의 부재를 재확인하며 daemon 오류나 불명을 성공으로 바꾸지 않는다.
- macOS/Linux x86_64 **Rust270개**, React79개·타입·production build·DTO·fmt·clippy를 통과했다.
  독립 검토, 실제 JS/PNG/private-resource/egress/rebinding 격리 시험과 종료 후 자원 부재를
  확인했다. 직접 실행한 실제 공개 페이지 읽기는 수정 후5/5이며1.959–3.425초였다.
- 실제 `gpt-6.1-terra/high`가 정상 검증된 동일 연결을 통해 채팅→브라우저→출처 있는
  답변까지2회 호출·8.649초에 완료했다. 새 연결의 proof가 응답 없이 끝나는 실패는
  별도로 남아 있으며, 해당 제한과 이전 실패를 검증 기록에 보존했다.
- 새 프로필4는 정확한 배포 digest를 고정한다. 기존1–3과 retry는 새 브라우저 권한을 얻지
  않는다. 메일 최종 미리보기·명시적 사용자 승인 규칙은 그대로다.

[설정과 사용 범위](browser-chat.md), [Linux 배포](../deployment/browser/README.md),
[검증·실패·수정 기록](reviews/2026-11-13-browser-chat.md)에 상세 내용을 둔다.
이 열한 번째 단위는 일회성 공개 페이지 읽기다. 아래 열세 번째 단위에서 개인 작업 공간의
화면 보기·사람의 로그인/조작·동일 화면 복귀를 추가했다. 모델의 범용 자동 클릭/폼은 아직 없다.

## 열두 번째 단위: 웹·메일 업무를 수행하는 절차

새 `web-research`·`inbox-briefing`·`mail-reply`를 추가해 내장 스킬이 여섯 가지다.
실제 도구 목록을 확인하고, 받은 URL/대상/기간을 재사용하며, 읽기→검토→필요한 계산이나
초안→실제 결과 설명으로 이어 간다. 새 권한이나 별도 일반 업무 분류기를 추가하지 않았다.

- 웹 조사는 HTTP의 빈칸에만 렌더링을 사용하고, 비용·배송비·세금·수량·통화를 맞춘다.
  메일 브리핑은 확인한 범위의 우선순위·기한·다음 행동을 정리한다. 답장은 실제 로컬
  새 메일 초안을 만들고 최종 미리보기·허락으로 연결한다. threading·첨부 지원은 아직 없다.
- 실제 `gpt-6.1-terra/high`와 합성 도구 응답을 사용한 네 업무의 첫 strict 결과는3/4였다.
  메일 요약의 날짜 범위 과장을 수정했고, 스킬 읽기 전 호출에도 적용되는 검색 설명을
  보완한 최종 재시험1/1을 통과했다. 실제 Gmail 발송이나 전체 비서 성능 평가가 아니다.
- macOS/Linux의 관련 pin/철회·실행·schema6개와 Linux 빌드·fmt·clippy를 통과했다.
  기존 스킬 bytes와 실행 pin을 보존했으며 독립 검토와 공개 검사도 수행했다.

[사용법](external-task-recipes.md)과 [실모델 시험·실패·수정 기록](reviews/2026-11-13-external-task-recipes.md)에 범위를 구분한다.

## 열세 번째 단위: 브라우저를 이어 보고 직접 조작하기

뮤즈의 전용 Linux 브라우저·broker·AX 관측과 피클의 직접 조작→완료→같은 화면 복귀를
참고해 [개인 작업 브라우저](browser-workspaces.md)를 채팅에 연결했다. Chromium 엔진은
유지하며 도구 호출마다 새 창을 버리는 방식에서 사용자·대화별 profile/page 유지로 확장했다.

- 새 profile5의 `browser_open`·`browser_observe`·`browser_handoff`와 React 화면 보기·
  직접 조작·완료·종료를 제공한다. 사람은 로그인/MFA·클릭·텍스트·키·스크롤·주소 이동을
  처리한다. 조작 중 모델 관측은 멈추고, 이후에도 모델의 인증된 workspace 이동은 차단한다.
- 완료 상태와 후속 사용자 turn을 한 transaction으로 저장한다. 같은 요청의 완료 재확인은
  같은 turn을 반환한다. 원래 run을 되살리는 것이 아니라 같은 대화·브라우저에서 새로 이어 간다.
- 앱 종료와 대화 stream 단절은 browser를 닫지 않는다. 30분 유휴/8시간 상한, 조작권5분 유휴,
  다른 탭 controller fence와 계정/대화별 격리를 적용한다. 재시작은 결과 불명이며 자동 재생하지 않는다.
- 관측은 AX와 보이는 layout text를 결합하고 editable/form 값을 제외한다. 사람에게만 전달하는
  일회용 frame proof로 다른 페이지/포커스/overlay를 오래된 좌표로 조작하지 않게 한다.
  화면·입력·token은 일반 대화/DB에 저장하지 않으며 사용자 키 볼트라고 주장하지 않는다.
- 브라우저 종료 drain·후속 turn 원자성·janitor 경쟁·실제 CDP 형식 차이를 독립 검토와 Linux
  시험에서 수정했다. 로그인 직후 screenshot 응답 정지를 재현해 캡처만 만료시키고 세션을
  보존한다. 화면 갱신은 재시도하지만 클릭·입력은 재실행하지 않는다.

- 최종 Linux Rust291개 통과(실모델 opt-in1개 제외), React91개·타입/production build·DTO·
  fmt/clippy를 확인했다. 실제 Linux smoke31개 명령과 로그인 직후12/12 반복 복구,
  합성 모델→실제 API/SQLite/Chromium의11가지 조건을 통과했다. 종료 후 시험 자원은0개다.
  화면 검증은1280×720/390×844 합성 component이며 실제 모바일 기기 시험과 구별한다.

공식 출처·실패·수정·최종 검증 범위는 [조사](research/browser-workspaces-2026-11-13.md)와
[수락 기록](reviews/2026-11-13-browser-workspaces.md)에 남긴다. 범용 자동 폼·vault 자동 로그인·
장기 profile 복구는 후속이며, 메일 최종 미리보기와 명시적인 발송 허락은 유지한다.

## 공식 Gmail·Slack과 커스텀 수집 연결

R29/F70의 계정별 수집 연결을 구현했다. 카톡 수집기는 F38 공개 배포 제외로 유지하며,
기존 개인 프로젝트·서비스·cron·DB는 변경하지 않았다. 별도 개인 producer를 generic HTTP
protocol로 연결할 수 있고, 공개 서버가 그 수집기의 코드나 설치 도구를 실행하지 않는다.

- SQLite migration9와 Rust core/store/server/agent에 개인 연결·고정 범위·credential epoch,
  실행 lease·heartbeat·원자 batch/영수증/checkpoint·원문 revision별 출처를 연결했다.
  로그인 세션을 가장하지 않는 background credential carrier를 사용한다.
- Gmail은 기존 개인 OAuth와 명시한 라벨/시작 날짜로 backfill/history를 읽는다. Slack은
  검증한 개인 user token으로 채널과 제한된 기간의 thread를 재확인한다. Slack의 polling 완료와
  커스텀 producer의 완료 보고를 전체 원격 자료 수집 보증으로 표시하지 않는다.
- 외부 수집기는 연결 하나의 만료 token으로 batch/status/receipt만 사용한다. 토큰 회전·철회,
  pause·disconnect, 계정 변경과 늦은 commit을 검사한다. 같은 요청의 재시도는 같은 영수증을
  반환한다. 부분 페이지에서 빠진 자료는 삭제하지 않으며 full snapshot은 계속 거절한다.
- `/settings/sources`에서 개인 연결과 이력을 관리하고 원문에서 연결/저장 영수증을 추적한다.
  원문 삭제는 의존 기억·근거를 철회하며 재수집으로 부활하지 않는다. 연결 해제 후 보관한
  자료도 나중에 삭제할 수 있다. source가 참조하는 receipt/run 메타데이터는 보존한다.
- macOS·Linux x86_64에서 **Rust318개 통과, 실모델 opt-in1개 제외**. React **113개**,
  타입/production build·생성 DTO·fmt/all-target clippy를 확인했다. source-specific 새 검증은
  Store14·provider9·HTTP2이며, 실제 CLI 시험에는 설정 화면 직접 접속을 추가했다.
- Linux 실제 loopback HTTP 여정 **12/12**: Rust producer2회 실행의 같은 영수증/sequence1,
  검색·출처, 계정 격리, pause/revoke, token/checkpoint 비노출을 확인했다. 종료 후 시험
  컨테이너·네트워크는0개다. 실제 Google/Slack/모델 호출과 운영 배포는 수행하지 않았다.
- 별도 macOS 서버 강제 종료→재시작 시험6개도 통과했다. 완료 전 run은 unknown으로 남고,
  이미 저장한 자료·출처·영수증·cursor는 유지되며 동일 요청을 다시 보내도 중복 저장하지 않는다.
- 독립 검토에서 권한 없는 앞쪽 연결이 후속 작업을 막는 스케줄러, 원문이 참조하는 영수증 삭제,
  Gmail SPAM/TRASH 범위 누락을 찾아 수정·재검토했다. 실제 브라우저에서 발견한 설정 주소404도
  수정하고 설치 회귀 시험을 추가했다. CI는 계속 비활성이다.

설치자와 계정별 Google 연결 절차는 [mail](mail.md), 프로토콜/현재 제한은
[data connections](data-connections.md), 구체적인 검증은
[수락 기록](reviews/2026-11-13-source-connections.md)에 있다. 공용 OAuth 중계 서비스와
Slack OAuth 앱 자동 설치는 제공하지 않는다. 실계정 opt-in 시험은 별도 미완료 항목으로 남긴다.

## Google 설치 설정 마법사

독립 셀프호스팅을 위한 `/admin/google`을 구현했다. 설치자는 Google Cloud 앱을 한 번
준비하고 콜백 복사·웹 클라이언트 JSON 확인/저장·본인 연결 확인을 진행한다. 다른 회원은
자기 설정에서 Google 로그인만 수행하며 메일함/토큰을 공유하지 않는다.

- SQLite migration10에 설치 설정을 암호화해 저장하고 재시작 없이 적용한다. 명시한
  운영자 파일 모드는 유지하며 웹 설정과 충돌하면 저장을 거절한다.
- 진행 중인 OAuth와 다른 회원의 기존 grant를 보존한다. 앱 secret 변경 전후의 늦은
  token refresh·오류·Gmail sync 결과를 차단하고 이미 모은 자료는 유지한다.
- 파일 검증과 실제 Google 연결 성공을 구별한다. 확인은 관리자의 본인 profile만
  대상으로 하며 메일 발송·수집을 시작하지 않는다. 발송 전 사용자 최종 승인은 유지한다.
- macOS·Linux x86_64 Rust332개(실모델 opt-in1개 제외)·React128개와
  타입/build/DTO·fmt/clippy를 통과했다. 실제 Chromium의 파일 등록·콜백 복사·모바일
  스크롤·서버 재시작 후 유지, Linux 실제 CLI/HTTP8가지 조건을 확인했다. 인증/저장
  경계를 독립 검토했다. 상세 검증 범위는 [수락 기록](reviews/2026-11-13-google-setup.md)에 있다.

[설치 방법](mail.md)을 따른다. Google 앱 생성·실계정 동의는 설치자가 수행하며 중앙
OAuth 서비스나 실계정 검증 완료를 주장하지 않는다. 운영 배포는 하지 않았고 CI는 꺼져 있다.

## PWA 설치와 공개 오프라인 화면

기본 채팅에서 시작하는 standalone manifest·픽셀 시종 아이콘·설치 메뉴를 추가했다.
지원 브라우저에서 홈 화면에 설치하며, 처음 등록한 공개 HTML·JS·CSS·아이콘만 캐시한다.
개인 API·대화·자료·암호를 캐시하거나 오프라인 전송 큐를 만들지 않는다.

- 연결이 끊겨도 열린 화면의 입력 초안을 유지하며 연결 상태를 알린다. 오프라인 시작은
  공개 연결 복구 화면을 표시하고, 연결이 돌아오면 서버의 현재 세션을 확인한다.
- 새 서비스 워커는 열린 탭을 강제로 새로고침하지 않는다. 모든 탭을 닫고 다시 열면
  새 버전을 활성화하고 이전 공개 캐시를 지운다.
- macOS React133개와 추가 재연결 회귀1개·타입·production build·CLI 내장 자산 검사를 통과했다. 실제 Chromium에서
  공개 파일만 있는 캐시, 연결 전후 초안 유지, 오프라인 시작과 업데이트 대기/활성화를 확인했다.
  원본1254px PNG가 설치 후보에서 제외되는 문제는 원본을 내장한 scalable SVG packaging으로
  수정하고 전용 일반 프로필에서 실제 설치 가능성 오류0개를 확인했다.
- 독립 검토에서 초기 로딩 중 재연결 이벤트가 사라지는 경쟁을 수정·재검토했다. Linux의
  내장 자산·HTTP 로그인/재시작 통합 검사를 통과했다. 실제 iOS/Android, 공유받기·미디어·
  Web Push 완료를 뜻하지 않는다. [사용 범위](pwa.md)와 [검증 기록](reviews/2026-11-13-pwa-shell.md)을 따른다.

## 아직 구현하지 않은 범위

구독 인증·개인 provider 연결, 추가 실모델 평가, 자연어 예약·예약된 모델 업무·캘린더·푸시,
일반 파일·PDF·녹음·PWA 공유받기, 공용 OAuth 연결 서비스·Slack OAuth 앱 설치, 인증된 사이트의 자동 조작·장기 브라우저 프로필 복구·배송/예약/전화/거래,
개인 학습 공유·자동 catalog 갱신, 독립 볼트 서비스·웹·credential broker,
자동 운영 백업·과거 복원·데이터 이전·일반 설치 패키지는 남아 있다. TOTP와 로컬 CSV
암호화는 native 코어까지 구현했다. 사용할 수 있는 볼트는 첫 출시의 필수 범위이며 사용자
연결 여부만 선택 사항이다. 이번 웹·브라우저·Gmail 구현을 전체 비서 완성으로 표시하지 않는다.

계정 복구 단위는 macOS·Linux x86_64 Rust340개(실모델 opt-in1개 제외)를 통과했고 독립 검토에서 발견한 초기화 부작용을 수정·재검토했다.

현재 auth journal과 DB 사이 crash gap은 해당 계정을 `recovery_required`로 잠근다. 중지한 설치의 로컬 `recover-account`는 계정 UUID·역할·새 비밀번호 파일을 명시해24시간 임시 credential로 재등록하며 최초 변경과 기존 세션 철회를 강제한다. 정상 계정의 임의 재설정이나 백업/복원 기능은 아니다. [중지된 설치의 암호화 export](backup.md)는 현재 DB·snapshot journal과 별도 운영자 recipient를 지원한다. 이후 추가한 [봉인된 현재 스냅샷 복원](restore.md)은 완료된 SQLite 이미지도 보존한다. 최신 독립 journal·과거 export 재생·일반 운영 이전은 남아 있으므로 이 개발 프리뷰를 운영 이전 완료로 취급하지 않는다. [사용법](development.md#recovering-a-journal-locked-account)과 [합성 검증 범위](reviews/2026-11-13-operator-account-recovery.md)를 따르며 `doctor`도 중지된 설치에만 사용한다.

## 다음 작업

1. P0의 남은 provider capability·baseline/fixture adapter·crypto/broker 위험 검증과 독립 복구 입력을 확인한다.
2. P1의 run/effect·dispatch·revision·사용량·복원 계약을 작은 저장 단위로 구현한다. 현재 계정 기반이 P1 전체 완료를 뜻하지 않는다.
3. 현재 요청에서 봉인한 명시적 기억 변경과 근거 철회는 연결했다. 수정/망각 대상 해석과 저장 경계를 보완했다. 별도 holdout 실모델 검사는 도구 연결 검증이 완료되지 않아 남아 있다. 자료 모음별 추출 설정·background 작업·예산과 기존 망각 억제를 연결하는 제한된 자동 추출도 구현했다. 실제 모델의 추출 정확도와 더 넓은 자동화는 후속 검증이다.
4. [migration](migration.md)의 순서에 따라 자료·기억→도구·스킬→예약·연결→미디어·볼트→이전·출시로 진행한다. P0/P1/P2와 미구현 OpenSpec change를 완료 처리하지 않는다.

모든 공개 소스는 새 이력과 합성 자료만 사용한다. 원본 운영 저장소·서비스·수집기·cron·DB는 보존한다. 개인 AGENTS/CLAUDE 및 editor 지침은 Git에서 제외하고 CONTRIBUTING·OpenSpec·제품 community SKILL은 공유한다.

## 유지되는 설계 결정

백업 manifest·첨부 pin·서버 밖 outer recovery key, DST/중단 예약 규칙, current epoch의 dispatch/철회 순서, 사용자 기기의 credential grant, 증거 없는 외부 작업 성공 금지와 평가의 not-run 구분은 그대로 유지한다. 계약 검토 기록은 [초기 검사](reviews/2026-11-12-initial-checks.md)와 [사용자 여정 검토](reviews/2026-11-12-user-journeys.md)에 있으며 실제 암호/복원/provider 시험과 구별한다.

## Community curated 설치 첫 단위 (2026-11-13)

제안·할 수 있는 일(`/app/skills`)에서 공개6개 instruction package의 사용자별 설치·갱신·rollback·비활성화·삭제·숨김을 SQLite0011에 저장한다. exact source commit+4-file package SHA로 고정하고 새 run에만 active revision을 적용한다. 설치는 외부 실행이 아니다. remove는 해당 사용자 관련 진행 run과 retry를 철회하고 완료 receipt는 보존한다. 실제 credential/tool 권한은 기존 run profile만 결정한다. 시작 버튼은 작성 중인 초안을 보존하며 전송하지 않는다. 화면 직접 접속도 지원한다.

Rust의 strict package bounds/형식/license/example/capability 검사, interrupted staging→update→rollback→remove와 두 사용자 분리, 실제 run pin·삭제 철회, HTTP 인증/계정 경계와 React 설치·연결 부족·숨김·늦은 계정 응답을 합성 자료로 검증했다. 이 첫 단위는 curated package를 대상으로 했다. 공개 GitHub 원본 추가는 아래 후속 단위에서 구현했으며 archive·release generator·자동 다운로드·private-learning export·예약 needs_skill·local edit/lockfile/GC는 아직 미구현이다. [community 계약](community-skills.md)과 [패키지 안내](../community/README.md)를 따른다.

커뮤니티 단위는 macOS·Linux x86_64 Rust344개(실모델 opt-in1개 제외), React139개·타입/production build와 독립 검토를 통과했다. 상세 범위와 후속 항목은 [검증 기록](reviews/2026-11-13-community-curated.md)에 남긴다.


## 기억 수정·망각 대상 해석

기존의 정확한 문장 일치에 더해, 표현이 달라 찾지 못한 대상은 현재 기억 후보에서 의미로 선택한다. 행동·새 사실은 현재 사용자 요청에서 먼저 봉인하고 모델은 후보 번호만 고른다. 모호하거나 후보가 한도를 넘으면 확인을 요청하며, 대상·근거·유효 시점이 바뀌면 수정하지 않는다. 망각한 후보는 저장된 선택 계획에서도 즉시 제거하고 재시도에서 다시 보내지 않는다.

macOS의 집중 Store31·agent21·schema/HTTP 검사와 Linux 전체362개(실모델 opt-in1개 제외), fmt/clippy/plan/OpenSpec와 독립 검토를 통과했다. 별도12문항 실모델 평가는 연결 검사의 빈 최종 응답이2회 재현되어 질문 전송0·미실행12다. 이전7/10 결과를 새 성공률로 바꾸지 않는다. [검증·제한](reviews/2026-11-13-memory-target-resolution.md)을 따른다.

## 중지된 설치의 암호화 백업 파일

Rust CLI의 `backup keygen`으로 별도 복구 키를 만들고 `backup setup/create`로 현재 SQLite·snapshot journal을 표준 age 암호화 tar 파일로 내보낸다. 개인 키는 별도0600 파일에만 쓰고 공개 recipient만 출력한다. 설치 UUID·공개 키 이력을 보존하고 WAL을 포함한 일관된 snapshot을 만들며 실행 중인 설치·잘못된 경로·덮어쓰기를 거절한다. 서버 provider key 없이도 export할 수 있다.

macOS 검사와 표준 Go age 복호화, Linux의 영향받는 저장/CLI 검사 및 독립 검토 범위를 [검증 기록](reviews/2026-11-13-stopped-encrypted-export.md)에 남긴다. Linux의 최초 fixture 권한 실패와0700 수정 후 재검증도 포함한다. 과거 백업 복원·서버 밖 최신 삭제 기록·미디어 복구는 후속 작업이다. 이 단위로 전체 백업/복원 계획을 완료 처리하지 않는다.

## 공개 GitHub community 원본 추가

수동 public GitHub root URL·디렉터리·ref 입력→고정 commit 미리보기→사용자별 추가/업데이트를 제공한다. Rust는 bounded commit/tree/blob와 decoded size/Git object identity를 확인하고 instruction4-file 형식·license·capability 검사를 재사용한다. 외부 identity는 repository+directory이며 표시 이름으로 bundled 절차를 덮어쓰지 않는다. 기존 curated pins는 유지한다. 다운로드 뒤 account 재검증과 activation CAS를 적용하고 preview/설치는 작업을 실행하지 않는다. private 소스·Git subprocess/archive/script·자동 갱신·export·GC는 아직 없다. [community 계약](community-skills.md)을 따른다.

공개 GitHub 추가 단위는 macOS·Linux의 집중 connector/저장/run-pin/HTTP/설치 검사와 React56개·생성 DTO·타입/build·clippy·독립 검토를 통과했다. 늦은 catalog 응답이 설치된 카드를 숨기던 문제를 수정하고 재현 검사로 확인했다. [검증 기록](reviews/2026-11-13-public-github-skills.md)을 따른다.

## 제한된 자료 자동 기억 추출

자료 모음별 opt-in과 표시된 작성자/지정한 사람 기준, 새 자료 revision 자동 처리와
기존 한 자료의 명시 처리, 실제 근거가 연결된 기억·출처 표시를 추가했다.
Store가 별도 background grant/lease를 발급하며 세션을 만들지 않는다. 일일 예산은
모델 호출10회·직렬화 요청24,000바이트·예약 출력4,000토큰이며 입력 토큰 수라고
표현하지 않는다. 한 작업/한 호출에서 완전한 문단만 선택하고 재시작 후 불명확한
원격 dispatch를 반복하지 않는다. 근거 철회·계정/설정 변경·정확한 인용 망각 억제를
commit 전에 검사한다. 원문 text나 모델 checkpoint는 작업 queue에 남기지 않는다.

검증 기록과 제한은 [automatic memory](automatic-memory.md)에 둔다. 아직 실모델
추출 precision/recall이나 모든 의미상 망각을 검증했다고 주장하지 않는다.

통합 Linux 전체392개(실모델·참조 도구·public-network opt-in4개 제외) 통과 후,
CRLF 문단 보완의 추출12개·worker1개·설치1개를 Linux에서 다시 확인했다.
macOS의 관련 기억/자료/백업 회귀, React31개, 생성 DTO·타입/build·clippy·fmt·계획·
OpenSpec와 독립 검토를 통과했다. 늦은 설정 응답과 긴 CRLF 이메일의 문단 처리를
수정했다. [검증 기록](reviews/2026-11-13-automatic-memory.md)을 따르며 실모델 추출
precision/recall은 아직 측정하지 않았다.


## 삭제·연결 해제의 중단 복구

migration0015부터 설치 UUID와 동결된 MSKNOW01 끝을 묶는 MSDSTR01을 사용한다.
기억/자료 삭제, 명령 forget, 대화 삭제, 메일 연결 해제, 최대32개 자료의 한 배치
삭제는 한 번의 append+fsync 뒤 효과·영수증·watermark를 같은 DB transaction에
반영한다. 실패한 actor는 재시작 replay 전까지 막힌다. 새 export는 format2의
고정5개 entry와 필수 coverage를 포함하며 기존 format1 codec은 보존한다.

합성 fault/replay와 구버전 codec 검증은 [검증 기록](reviews/2026-11-13-destructive-intents.md),
정확한 범위·재시도·한계는 [계약](recovery-intents.md)을 따른다. 자동 기억14와
합친 schema15의 macOS·Linux 검사와 독립 검토를 마쳤다. Linux 최초 실행의
fixture 파일 권한1건을 수정한 뒤 영향받는 검사와 doc-tests도 통과했다. 과거
복원·외부 최신 checkpoint·기존 미기록 삭제의 소급 복구는 구현하지 않았다.

## Community 로컬 작성 CLI

`moosuri skills init <slug> [--output DIRECTORY]`와 `moosuri skills check DIRECTORY`로 설치 없이 네 파일 instruction template 작성과 호환성 검사를 제공한다. 합성 정상/지원 불가 예제와 Apache-2.0 전문을 생성하고 installer의 기존 content validator를 그대로 공유한다. 일반 repository 작성은 허용하며 기존 목적지·링크·special/executable 파일·크기 초과·app-data 경로를 거절한다. init은 완성된 staging을 no-replace로 게시하고 알 수 없는 tree를 정리하지 않는다.

이 단위는 source 작성·호환성 검사다. contributor의 편집·검토·commit/push는 명시적 별도 행동이며 설치 UI의 immutable GitHub pin과 native 도구 권한은 그대로다. release/catalog generator·private-learning export·설치본 직접 수정·GC는 미구현이다. 실제 합성 검사와 제한은 [검증 기록](reviews/2026-11-13-community-authoring.md)에 남긴다. 독립 검토와 macOS 통합 CLI5개, Linux Store3개·CLI13개(참조 도구 opt-in1개 제외)를 통과했다. Linux no-replace 게시·동시 init·기존 backup/recovery/설치 경로도 확인했다.

## 개인 문서·목표·요청 기록

[개인 기록](artifacts.md)을 schema16으로 통합했다. 같은 ID의 편집·버전·저장 영수증,
대화에서 현재 지시로 작성/수정, 근거 철회와 삭제 복구를 제공한다. 저장과 고정 완료
메시지를 한 transaction으로 묶어 중단 후 문서를 다시 생성하지 않는다. 편집 초안은
충돌 시 보존하며 날짜는 기기의 현지 시각으로 입력한다.

독립 검토와 수정 재검토, 최종 Store10·agent4·HTTP2, Linux 관련25개 검사와 실제
Chromium의 생성/수정/현지 날짜/모바일 스크롤을 확인했다. React 관련48개와 최종
화면 수정 후27개가 통과했다. 합성 검사와 실제 모델의 문서 품질 평가는 구별한다.
이 단위는 F15와 F28의 기록/briefing 부분이다. 미래 예약·알림·자동 약속 감지는
[검증 기록](reviews/2026-11-13-personal-artifacts.md)의 후속 범위로 남는다.

## 로컬 알림 예약·소식함

고정 내용을 계정 소식함에 전달하는 예약을 schema17으로 통합했다. UTC 일회·고정
기준 간격·IANA 시간대 매일 규칙, 일시 정지/다시 시작/편집/삭제, 회차·알림 영수증
중복 방지, 읽음/숨김·조용한 시간 상태와5초 worker를 제공한다. 모델 호출 없이
실행하며 대화 초안을 유지하고 예약 화면으로 이동할 수 있다.

독립 검토와 통합 macOS·Linux 검사에서 Store9·migration7·agent59·HTTP worker/reopen1을
통과했다. Linux 설치1, React/App/public-shell32, 타입/build·DTO·clippy도 통과했다.
이 첫 단위 이후의 대화 알림·매주 규칙은 아래 schema22 단위에서 연결했다. 모델 업무·변경만 알림·Push는 후속 범위다. 계정 정지는 현재 active 상태와 auth epoch로 차단한다.
복원 기능에는 별도 quarantine hook을 연결한다.
[현재 범위](schedules.md)와 [검증 기록](reviews/2026-11-13-local-reminders.md)을 따른다.
주기 조회 중에도 입력·저장이 가능하며, 늦은 조회가 저장 결과를 덮어쓰지 않는다.
이 수정 뒤 예약 UI8개와 타입/production build를 다시 확인했다.

## 계정 관리 수명

People의 Manage에서 정지·재개·역할 변경·24시간 임시 암호 재발급·전체 기기
로그아웃을 제공한다. 각 변경은 현재 관리자와 대상 revision을 다시 검사하고
auth epoch·세션 철회·내용 없는 영수증을 함께 저장한다. 재개해도 이전 세션이나
background 권한은 되살리지 않는다. 저장된 개인 내용은 보존하며 관리자에게
열람 API를 주지 않는다. 암호 해싱은 전달 잠금 밖에서 진행한다.

동일 요청 재시도는 저장한 영수증만 돌려주며 암호를 중복 생성하거나 다시 표시하지
않는다. 독립 암호 변경·관리자 강등·계정 전환·동시 클릭·이전 목록 응답을 검사한다.
저장 중단의 auth journal replay는 계정을 잠그며, 검증된 기록도 fsync 뒤에만
재생 권한으로 사용한다. 자세한 범위는 [계정 관리](account-management.md)에 있다.

schema18로 통합하고 독립 검토의 임시 비밀번호 표시 경합을 수정·재검증했다.
macOS Store204와 추가 예약/정지 회귀1, HTTP33·CLI7·React36·타입/build·clippy를
통과했고 Linux 관련64개 검사가 통과했다. [검증 기록](reviews/2026-11-13-account-management.md)에
범위를 남겼다. 전체 계정 삭제·초대 메일·볼트 철회는 후속 작업이다.


## 봉인된 현재 스냅샷 이전·복원

schema19/20의 checkpoint·restore를 통합했다. 중지한 설치를 영구 봉인하고
현재 SQLite·세 저널을 표준 age로 내보낸다. 별도로 신뢰한 head와 복구 키로
빈 목적지에만 복원하며, 모든 계정은 새 임시 암호로 재등록한다. 이전 세션·
provider/메일/자료 credential과 미완료 작업은 다시 실행되지 않고 예약은 정지한다.
삭제·근거 철회·문서·목표·완료 영수증·사용량과 계정 정지는 보존한다.

독립 검토의 늦은 rename/fsync 오류를 수정했다. 새 실행·계정 복구·오프라인
writer는 잠금을 잡은 채 부모 디렉터리 동기화에 성공해야 진행한다. 같은 입력의
복원 재시도는 이후 암호 변경·새 기억을 덮어쓰지 않는다. 최종 Linux 관련52개,
macOS cutover18개와 참조 Go age1개, clippy/fmt/계획/OpenSpec을 확인했다.
실제 프로세스 종료·오류 주입 검증이며 물리적 정전 실험은 아니다.

[복원 사용법](restore.md)과 [검증 기록](reviews/2026-11-13-sealed-restore.md)을 따른다.
과거 export의 임의 재생·자동 외부 백업·서버 유실 뒤의 독립 최신성 증명·운영
설치의 실제 이전은 별도 범위다. CI는 계속 비활성이다.

## 사용자 기기용 볼트 암호화 코어

Rust v2 암호화·복구 키·암호 변경/루트 회전·TOTP·로컬 Apple CSV 필터/암호화
코어를 통합했다. 항목 제목·아이디·URL·암호·TOTP seed를 모두 암호화하고
계정 관리자 비밀번호 재설정으로 볼트를 열 수 있는 경로는 제공하지 않는다.
이 단위는 라이브러리이며, 아직 볼트 서버·웹 화면·credential broker는 아니다.

독립 검토와 별도 Python 구현의 암호화/TOTP 교차 검증을 통과했다. macOS·Linux에서
코어13개와 금지된 API 사용의 compile-fail4개를 확인했다. 기존 lock 항목 변경 없이
검토한 공개 dependency12개만 추가했다. WASM/Worker·안전한 암호문 저장/복원·
구버전 이전·자동 credential 사용은 남아 있다. [정확한 범위](vault-core.md)와
[검증 기록](reviews/2026-11-13-vault-core.md)을 따른다.

## 이미지 첨부·미리보기·보관함

schema21에 PNG/JPEG/WebP 준비·독립 업로드·파일별 취소/재시도와 실제 composer/
대화/보관함 미리보기·확대·다운로드·삭제를 연결했다. 글을 보내도 미완료 업로드는
남으며, 보낸 메시지와 재시도는 동일한 준비된 이미지를 사용한다. 실제 픽셀 검사를
통과한 model/effort만 이미지를 받는다. 이미지 문맥의 도구·메모리/문서 변경은 제한한다.

삭제 후 늦은 다운로드가 완료되는 경쟁을 독립 검토에서 수정·재검증했다. 완료된
이미지는 SQLite 스냅샷에 포함하고 복원 시 미완료 업로드·vision proof는 격리한다.
Linux 관련482개와 macOS React196개·타입/build·clippy·DTO를 통과했다. 실제 Chrome에서
발견한 CSP 미리보기 차단을 별도로 수정·독립 검토하고 Linux CLI18개를 재확인했다.

390×844 Chrome에서 합성 이미지 업로드·확대·재시작 후 보관·삭제·초안 유지·입력 영역
스크롤을 확인했다. 실모델 이미지 품질과 실제 모바일 기기는 아직 검증하지 않았다.
일반 파일·PDF·STT·녹음·PWA 공유받기는 후속 범위다. [사용법](media.md)과
[검증 기록](reviews/2026-11-13-private-images.md)에 정확한 범위를 남겼다.


## 대화에서 고정 알림 등록·변경

schema22에 한글·영문 시간/주간 반복 해석과 생성·내용/시간 변경·일시정지·재개·
삭제를 연결했다. 실제 저장 결과와 대화 완료를 한 transaction으로 확정하고,
응답 유실 재시도는 원래 시간대·선택을 유지해 중복 등록을 막는다. 선택한 알림은
확인된 변경 결과를 따라가므로 연이어 끄고 켤 수 있으며 다른 선택을 덮지 않는다.

현재 발화의 명확한 고정 알림만 처리한다. 조사·메일 발송 같은 예약 업무를
등록한 것처럼 말하지 않으며, 조건문·인용문·이미지 문맥도 알림 변경 권한이 아니다.
화면에는 저장된 결과와 현지 시각을 보여 준다. 아직 범용 의미 해석이나 다중 대화
확인 흐름은 아니며 검증된 모델 선택이 채팅 진입 조건으로 남아 있다.

독립 검토에서 조건부 삭제·예약 조사와 단순 알림 구별·본문의 요약/번역 단어·
한국어 ISO 날짜 회귀를 수정했다. 이어 실제 Chrome의 선택 revision 갱신 문제와
일상적인 켜기/끄기 표현을 보완했다. 플랫폼·브라우저·재시도·이미지/복원 검증의
실제 범위는 [검증 기록](reviews/2026-11-13-conversational-reminders.md)에 남긴다.
CI는 계속 비활성이고 실모델 성능이나 실제 기기 푸시 성적을 주장하지 않는다.
