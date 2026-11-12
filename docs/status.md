# 현재 상태

2026-11-12. 실제 구현·검증과 다음 작업은 이 문서가 정본이다. 기능 계약은 [features](features.md), 상세 단계는 [plan](plan.md)을 따른다.

공개 저장소는 [codemoo/moosuri](https://github.com/codemoo/moosuri)다. **계정·모델 설정·텍스트 대화와 개인 자료·기억 관리 기능을 구현한 개발 프리뷰다. 채팅의 자료 활용·자동 기억 추출·외부 작업은 아직 구현 중이다.** GitHub Actions/CI는 사용자 지시로 꺼져 있다. 작업 단위마다 로컬 검증 후 커밋·푸시한다.

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
추가했다. 이 단위는 library이며 웹 채팅은 여전히 text path를 사용한다.

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

## 아직 구현하지 않은 범위

구독 인증·개인 provider 연결, 도구를 사용하는 에이전트 하네스·자동 기억 추출·문서/목표·평가, 예약·알림·푸시, 첨부·녹음·PWA, 수집 connector, Google/외부 작업, community 스킬, 독립 볼트·TOTP·credential broker, 백업·복원·데이터 이전·일반 설치 패키지는 남아 있다. 볼트는 v1 제공 범위이며 사용자 연결 여부만 선택 사항이다.

현재 auth journal과 DB 사이 crash gap은 해당 계정을 `recovery_required`로 잠근다. 이를 해제할 operator 복구 경로와 백업/복원은 아직 없으므로 운영 계정·자료를 이 개발 프리뷰로 옮기지 않는다. `doctor`는 중지된 설치에만 사용한다.

## 다음 작업

1. P0의 남은 provider capability·baseline/fixture adapter·crypto/broker 위험 검증과 독립 복구 입력을 확인한다.
2. P1의 run/effect·dispatch·revision·사용량·복원 계약을 작은 저장 단위로 구현한다. 현재 계정 기반이 P1 전체 완료를 뜻하지 않는다.
3. 구현한 개인 자료 입력·검색·기억 수정/망각을 바탕으로, 저장되는 모델 단계·도구 영수증·근거 철회 계약을 추가하고 검색 도구와 명시적 기억 저장을 채팅에 연결한다. 자동 추출은 그다음이다.
4. [migration](migration.md)의 순서에 따라 자료·기억→도구·스킬→예약·연결→미디어·볼트→이전·출시로 진행한다. P0/P1/P2와 미구현 OpenSpec change를 완료 처리하지 않는다.

모든 공개 소스는 새 이력과 합성 자료만 사용한다. 원본 운영 저장소·서비스·수집기·cron·DB는 보존한다. 개인 AGENTS/CLAUDE 및 editor 지침은 Git에서 제외하고 CONTRIBUTING·OpenSpec·제품 community SKILL은 공유한다.

## 유지되는 설계 결정

백업 manifest·첨부 pin·서버 밖 outer recovery key, DST/중단 예약 규칙, current epoch의 dispatch/철회 순서, 사용자 기기의 credential grant, 증거 없는 외부 작업 성공 금지와 평가의 not-run 구분은 그대로 유지한다. 계약 검토 기록은 [초기 검사](reviews/2026-11-12-initial-checks.md)와 [사용자 여정 검토](reviews/2026-11-12-user-journeys.md)에 있으며 실제 암호/복원/provider 시험과 구별한다.
