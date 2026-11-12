# 현재 상태

2026-11-13. 실제 구현·검증과 다음 작업은 이 문서가 정본이다. 기능 계약은 [features](features.md), 상세 단계는 [plan](plan.md)을 따른다.

공개 저장소는 [codemoo/moosuri](https://github.com/codemoo/moosuri)다. **계정·모델·저장되는 대화·개인 자료/기억·업무 스킬에 공개 HTTP·격리 Chromium 조사와 개인 Gmail을 연결한 개발 프리뷰다. 메일은 최종 내용을 사용자가 승인한 뒤에만 발송한다. 자동 기억 추출·예약·인증된 브라우저 작업은 개발 중이다.** GitHub Actions/CI는 사용자 지시로 꺼져 있다. 작업 단위마다 로컬 검증 후 커밋·푸시한다.

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

기존 테스트는 복사하지 않았다. 새 테스트도 전체 제품 평가 성적으로 사용하지 않는다. source523·entrypoint108·R28/F69 조사표, 67개 planned 평가 묶음과 과거 검토 기록은 유지한다.

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
  저장한다. 대상 수정/삭제는 여전히 어휘 일치이며 의미 검색은 구현하지 않았다.
- 분류 호출 중앙값은4.834초,10개 대화 시나리오의 완료 중앙값은5.846초였다.
  전자는 app-server 시작 시간을, 후자는 임시 bridge를 포함하므로 제품 스트리밍
  지연이나 TTFT 측정으로 해석하지 않는다.

수정 대상 해석은 남은 품질 결함이다. 실행 구성·모든 점수·제한과 재실행 방법은
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
인증된 사이트 조작·로그인 인계·클릭/폼·채팅 스크린샷은 아직 제공하지 않는다.

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

## 아직 구현하지 않은 범위

구독 인증·개인 provider 연결, 자동 기억 추출·문서/목표·평가, 예약·캘린더·알림·푸시,
첨부·녹음·PWA, 기존 수집 connector, 인증된 브라우저 작업·배송/예약/전화/거래,
community 설치·업데이트·제안 카드, 독립 볼트·TOTP·credential broker,
백업·복원·데이터 이전·일반 설치 패키지는 남아 있다. 볼트는 v1 제공 범위이며 사용자
연결 여부만 선택 사항이다. 이번 웹·브라우저·Gmail 구현을 전체 비서 완성으로 표시하지 않는다.

현재 auth journal과 DB 사이 crash gap은 해당 계정을 `recovery_required`로 잠근다. 이를 해제할 operator 복구 경로와 백업/복원은 아직 없으므로 운영 계정·자료를 이 개발 프리뷰로 옮기지 않는다. `doctor`는 중지된 설치에만 사용한다.

## 다음 작업

1. P0의 남은 provider capability·baseline/fixture adapter·crypto/broker 위험 검증과 독립 복구 입력을 확인한다.
2. P1의 run/effect·dispatch·revision·사용량·복원 계약을 작은 저장 단위로 구현한다. 현재 계정 기반이 P1 전체 완료를 뜻하지 않는다.
3. 현재 요청에서 봉인한 명시적 기억 변경과 근거 철회는 연결했다. 실모델에서 드러난 수정/망각 대상 해석 결함을 먼저 보완하고, 부정·동명이인·유사한 여러 기억과 미공개 holdout으로 오수정/오삭제를 검사한다. 그다음 자료 모음별 추출 설정·background 작업·예산과 기존 망각 억제를 연결하는 제한된 자동 추출을 진행한다.
4. [migration](migration.md)의 순서에 따라 자료·기억→도구·스킬→예약·연결→미디어·볼트→이전·출시로 진행한다. P0/P1/P2와 미구현 OpenSpec change를 완료 처리하지 않는다.

모든 공개 소스는 새 이력과 합성 자료만 사용한다. 원본 운영 저장소·서비스·수집기·cron·DB는 보존한다. 개인 AGENTS/CLAUDE 및 editor 지침은 Git에서 제외하고 CONTRIBUTING·OpenSpec·제품 community SKILL은 공유한다.

## 유지되는 설계 결정

백업 manifest·첨부 pin·서버 밖 outer recovery key, DST/중단 예약 규칙, current epoch의 dispatch/철회 순서, 사용자 기기의 credential grant, 증거 없는 외부 작업 성공 금지와 평가의 not-run 구분은 그대로 유지한다. 계약 검토 기록은 [초기 검사](reviews/2026-11-12-initial-checks.md)와 [사용자 여정 검토](reviews/2026-11-12-user-journeys.md)에 있으며 실제 암호/복원/provider 시험과 구별한다.
