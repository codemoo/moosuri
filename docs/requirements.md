# 확정 요구사항과 추적

2026-11-13 데이터 수집 범위 변경(R29)까지 반영한다. 구현 방식이 충돌하는 부분은 최신 지시가 우선한다. 기존 비서의 기능·UX·복구 요구는 계속 유지한다. 아래 항목은 요구 계약이며 실제 구현 상태는 [status](status.md)를 따른다.

| ID | 사용자 요구 | 최종 계획에서의 위치 | 검증 기준 |
| --- | --- | --- | --- |
| R01 | 이름 `moosuri`, dev 아래 새 폴더, GitHub 공개 준비 | README, audit, public-release | 새 source/history, 개인 원본 반입0, 폴더와 계획 검증 |
| R02 | Rust 재작성 | architecture | backend·agent·CLI·수집·이전 도구 Rust. 웹은 R03의 명시적 예외 |
| R03 | 관리자·사용자 PWA 등 모든 웹 React | architecture, features F09/F16/F56 | React+TypeScript admin/app/vault, Python/Node backend 없음; Node는 웹 build/test 도구 |
| R04 | 설치 간편, 설치 중 LLM 연결·기본 모델 | onboarding, providers | init→admin→연결→model/effort→접속, API/지원 구독/local 선택 |
| R05 | LLM은 공용, 가족·친구 계정 여러 개 | accounts-isolation, providers | shared profile/한도·fair queue, 대화/메모리/credential 혼합0; 구독 공유는 실제 provider 범위 내 |
| R06 | 계정은 이름·이메일, 임시 비번·첫 로그인 강제 변경 | onboarding, accounts-isolation | 초대/만료/일회·force-change 제한 session·rate limit·재발급/revoke |
| R07 | 최초 관리자 하나, 비번 입력 또는 생략시 임시 비번 | onboarding | 최초 bootstrap 경합에서도 admin1, skip이면 안전 난수·1회표시·강제변경 |
| R08 | 추가 계정은 원할 때 생성·배포 | onboarding, admin UX | public self-signup 기본 꺼짐, 선택 초대·resend/disable/delete lifecycle |
| R09 | Google 연결·gws 이메일 발송 | providers, migration P6 | 설치자 선택 sender·Gmail 최소 scope·gws isolated profile·invite receipt·재발송 중복0 |
| R10 | 설치 후 웹 대시보드·관리자 페이지 | onboarding, features F56 | app/admin 라우트·role 검사·bootstrap URL·상태/설정/사용자 화면 |
| R11 | 관리자 초대·모델·subscription/API 변경 | providers, accounts-isolation | config revision, 새 실행 적용·기존실행 bind·긴급폐기 별도, secret 재노출0 |
| R12 | 사용자별 사용량 | accounts-isolation, features F57 | run/step/user/profile별 ledger·input/cache/output·오류/추정·비용과 subscription quota 구분 |
| R13 | Tailscale 또는 일반 self-host | onboarding, public-release | private Serve vs public HTTPS, app 로그인 유지, callback/reachability·Push/vault origin 검사 |
| R14 | 관리자가 source/DB를 봐도 볼트 내용을 알 수 없음 | vault-security, F44~F51 | 사용자만 가진 master/recovery key, no escrow·계정 reset과 decrypt 분리, ciphertext/metadata만 서버 |
| R15 | SQLite 기본, PostgreSQL 등은 나중 | architecture, migration P1/P9 | single-host WAL·transaction claim·fence·SQLite backup; 설치에 외부 DB 불필요 |
| R16 | 사용자별 분리 환경과 세션 유지 | accounts-isolation | 모든 객체·event·worker·remote session·tempdir tenant binding, restart·logout·switch·revoke 회귀 |
| R17 | OpenAI/Claude 외 OpenRouter | providers | 직접 API·모델 catalog/capability·usage·router/privacy 설정·same tenant tools |
| R18 | 로컬 LLM 사용자 지원 | providers | Ollama/LM Studio/vLLM/compatible endpoint, LAN/Tailscale 주소·컨테이너 localhost·auth·offline 동작 |
| R19 | 지금까지 비서 기능 누락 금지; R29의 카톡 공개 배포 제외를 반영 | features F01~F64/F70, data-connections | 대화·첨부·녹음·기억·학습·예약·메일·수집·푸시·볼트·복구 모두 trace |
| R20 | 과도한 상호 승인 요구 금지 | architecture, accounts-isolation | 정상 조회/저장/명시된 작업은 즉시, admin 초기연결/실제 모호한 값만 입력 |
| R21 | 전수 조사·죽은/불필요한 항목 제거 | audit | 523개 파일 모두 판정, 실제 dead block 증거, active/optional과 dead 구분 |
| R22 | 단계별 계획, 코드 이전·재작성 순서 | migration | 각 단계 source→target→test→cutover 조건, 미구현과 검증 완료 구별 |

R03 때문에 전면 WASM UI 제안은 폐기한다. R05 때문에 단일 소유자 설치 제안은 폐기한다. R15 때문에 PostgreSQL 기본안은 폐기한다. 이는 기존 사용자 기능을 삭제하는 변경이 아니라 새 요구사항에 맞춘 구성 변경이다. SQLite의 C engine과 React/TS toolchain은 명시적으로 선택한 의존성이므로 전체 dependency가 Rust라는 표현을 사용하지 않는다.

공유 API/local은 기본 동작으로 설계한다. 구독 연결은 기능 목록에서 누락하지 않되 공급자별 공식 지원 범위를 표시한다. ChatGPT 계획 사용은 사용자/워크스페이스별 동의, Claude 구독은 허용된 별도 공식 실행 경로와 API 경로를 구분한다. 하나의 소비자 구독을 여러 계정의 공용 API로 만들 수 있다고 약속하지 않는다.

## 추가 평가·기여·자동 credential 요구

| ID | 사용자 요구 | 최종 계획에서의 위치 | 검증 기준 |
| --- | --- | --- | --- |
| R23 | 메모리·대화 성능·미션·스킬 등 테스트 시나리오와 검증법 | testing, test-examples, evaluation-scenarios | 독립 oracle·실제 runner/SQLite·합성 model 평가·fault/device/live 구분, 목표와 실측 구별 |
| R24 | 기존 테스트를 그대로 옮기지 않음 | testing, audit | 기존112 test/support는108 reference·4 exclude, 새 검사는 사용자 결과/결함으로 선정·중복 통합 |
| R25 | contributor 공동 개발·OpenSpec 도입 | CONTRIBUTING, openspec | 고정 CLI·spec/change 분리·작은 PR/작업 소유·로컬 검증, 초기 CI 비활성·재활성화는 별도 지시, 미구현을 완료 처리0 |
| R26 | community 스킬 개발·공유·설치를 간단히 | community-skills, F66/F67 | SKILL.md+필요 sidecar·Git pin·개인 설치/update/rollback/remove·명시적 합성 export |
| R27 | Git에 공유하고 기존 제안/할 수 있는 일로 노출 | community-skills, F68 | Git→public catalog→로컬 추천→기존 카드, 별도 marketplace 계정 불필요·준비 상태 정확 |
| R28 | 작업에 vault credential 연결·이미 사용한 정보 자동 재사용 | credential-use, F69 | 유효 tenant/service binding 자동 사용·최초/모호한 선택만 입력, secret 모델 비노출·offline/revoke/lost ACK 복구 |

## 데이터 수집 배포 범위 변경

| ID | 사용자 요구 | 최종 계획에서의 위치 | 검증 기준 |
| --- | --- | --- | --- |
| R29 | 카톡 수집기는 공개 배포 제외·개인 외부 수집기로 유지; 공식 메일/Slack과 커스텀 연결의 출처·상태 추적 | data-connections, F23/F34/F38/F39/F70, build-source-connections | public Kakao code/installer0; official/custom와 실제 가용성 분리, 연결→원문→batch receipt·scope/cursor 추적, tenant/revoke/replay/partial/heartbeat 검증 |

이 요구는 카톡 수집기의 공개 Rust 재작성·동봉 계획을 대체한다. 개인 수집·기존 운영은 보존하고,
공개 제품에는 독립 수집기가 사용할 공통 프로토콜만 제공한다. R19의 누락 금지와 충돌하지 않게
F38을 명시적 제외로 남기고 새 공통 기능은 F70으로 추적한다.
