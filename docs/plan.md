# moosuri Rust 재작성 계획

2026-11-13 범위 갱신. 기존 개인 비서를 누구나 설치할 수 있는 공개 프로젝트로 재설계한다. 이 문서는 제품 구현 완료 보고서가 아니다. 파일별 조사 결과는 [audit](audit.md), 현재 진행은 [status](status.md)가 정본이다.

## 목표와 완료 기준

`moosuri`는 대화를 통해 개인 자료를 찾고, 기억을 정정하고, 일을 맡기고, 주기적으로 실행하며, 실제 결과를 확인해 알려주는 비서다. 설치자가 자기 모델 키와 연결 계정을 설정한다. 개발자의 계정·Mac·서버·도메인·기억이 없어도 시작할 수 있어야 한다.

첫 공개 안정판은 다음 조건을 모두 만족해야 한다.

1. 깨끗한 Linux 환경에서 문서에 나온 한 가지 설치 경로로 시작하고, 개인 자료 연결 없이도 설정한 LLM으로 텍스트 대화·직접 기억·문서·알림을 사용할 수 있다. LLM 미연결 상태에서도 설치·계정·관리 화면은 열린다.
2. 서버·에이전트·CLI·모델 연결·수집 adapter·이전 도구는 Rust로, 사용자·관리자·볼트/PWA 웹은 React/TypeScript로 작성한다. SQLite를 기본으로 사용하며 Node는 웹 빌드/시험 도구다. Python/LangGraph 서버는 이관하지 않는다. 의존성 범위는 [architecture](architecture.md)에 명시한다.
3. [기능표](features.md)의 `기본` 항목은 모두 검증한다. `선택`은 설치/연결 여부의 선택이다. 볼트 F44~F51/F63·broker F69도 v1에 제공하고 실제 패키지·암호/복구·지원 adapter 시험을 통과해야 한다. 외부 연결은 지원 범위·연결 전 상태·설치 방법·합성/실계정 검증 수준을 정확히 표시한다. `개인 전용/후속`은 별도 표로 남겨 누락이나 완료로 숨기지 않는다.
4. 앱 종료·worker 재시작·응답 유실·재시도에서 같은 요청을 이어가며, 완료 영수증 없는 외부 작업을 성공으로 말하지 않는다. 삭제 자료와 볼트 평문이 다시 나타나지 않으며, 빈 환경 복원에서도 보존한 사진·첨부·산출물이 유지된다.
5. 공개 파일·Git 이력·빌드 결과에 개인 원문/credential/운영 주소가 없고, 설치·업데이트·백업·복원·제거·기여 절차를 따라 실행할 수 있다. 백업 복구 키·최신 journal·검증 자료는 원래 서버와 복원 대상 vault 없이 사용할 수 있어야 한다.

초기 계획 작업의 산출물은 새 폴더, 523개 파일의 처분 목록, API/도구 진입점 목록, 기존 테스트 선별 정책과 새 평가 시나리오, OpenSpec/기여 기반, community 카드·credential 자동 사용 설계, 단계·출시 조건과 Rust 검증기다. 기존 운영 소스를 삭제하거나 새 제품이 완성된 것으로 표시하지 않는다.

## 이전 방식

소스 디렉터리를 복사하거나 Python을 Rust 문법으로 옮기는 방식은 쓰지 않는다. **동작 계약 → 합성 입력/관측 결과 → Rust 구현 → 동일 계약 검증 → 제한 전환** 순서로 이전한다. 기존 코드의 모듈 이름은 조사 좌표이며 새 crate 구획을 강제하지 않는다.

기존 핵심 모듈은 서로 연결돼 있고 CLI·동적 도구·배포 진입점도 있다. import 수가 적다는 이유로 삭제하지 않는다. 현재 agent 모듈71개에서 통째로 버려도 되는 명백한 미사용 모듈은 확인하지 못했다. 대신 다음을 정리한다.

- 같은 의미를 중복 구현한 요청·revision·실행 제어는 타입과 공통 저장 계약으로 통합한다.
- 개인 환경 설치 스크립트는 일반 CLI·배포 템플릿으로 다시 만든다.
- 실험용 후보 엔진·오래된 빌드 도구·템플릿 hosting 설정은 제품에 넣지 않는다.
- 과거 배포 문서·모델 응답·운영 증거는 기능 근거만 요약한다. 전부 공개 저장소에 이관하지 않는다.
- 수집은 공식 Gmail·Slack adapter와 외부 custom HTTP protocol로 나눈다. 카톡 수집기는 공개 배포에서 제외하고 개인 프로젝트에서 공통 protocol로 연결한다. [R29 수집 계약](data-connections.md)을 따른다.

## 제품 구획

| 구획 | 포함하는 책임 | 첫 사용자 경험 |
| --- | --- | --- |
| 설치·설정 | 최초 admin 하나, 계정 초대·강제 비번 변경, private 경로, 모델/effort, 연결, 진단, 업데이트 | `moosuri init` 후 웹에서 설정 |
| 대화 | 기록, 첨부, 스트림, 취소, 재시도, 삭제 | 처음 열면 마지막 대화, 입력 초안 보존 |
| 기억·자료 | 원문 provenance, 검색, 현재 사실, 정정·망각, 자동 추출 | 근거를 보고 수정/삭제할 수 있음 |
| 에이전트 | bounded loop, 도구 선택, 체크포인트, 예산, 완료 검증 | 명확한 요청은 실행하고 막힌 값만 질문 |
| 맡긴 일·예약 | 목표, 지속 작업, 회차, 알림, 변경 비교, 조용한 시간 | 대화로 맡기고 카드에서 정지·재개 |
| 외부 연결·수집 | 공식 mail/Slack·custom ingest, 출처·sync 이력, 초안/발송/일정 영수증 | 공식/커스텀과 실제 준비 상태를 구별, 원문에서 연결 추적 |
| 암호 볼트 | 별도 origin·DB·세션, 기기 암호화, 가져오기·TOTP·trusted credential broker | 연결한 계정을 작업에 자동 재사용·잠금 시 대기 |
| community·기여 | GitHub/OpenSpec, 스킬 pin/update/rollback·합성 export | 기존 제안/할 수 있는 일에서 추가·연결·실행 |

기본은 한 설치 안에서 가족·친구 계정을 필요할 때 추가하는 self-host 서비스다. LLM/API/local 추론 용량을 공유하되 대화·기억·학습·원문·Google 인증·세션·실행 디렉터리는 사용자별이다. 관리자는 계정·공용 모델·사용량을 운영하며 개인 자료 열람 기능은 갖지 않는다. 상세는 [계정 격리](accounts-isolation.md), [모델 연결](providers.md), [설치](onboarding.md)를 따른다. 공개 SaaS 과금/조직 간 공유·임의 코드 설치·자동 구매 봇은 v1 필수 범위가 아니다.

## 단계별 진행

| 단계 | 산출물 | 종료 조건 |
| --- | --- | --- |
| P0 경계와 공개 기반 | crate 후보 spike, 공통 타입, OpenSpec/기여 CI·공개 개발 repo 준비·라이선스/의존성 정책, 새 합성 평가 규격·재현 baseline 연결·broker 경계 spike, React/PWA·볼트 worker·provider capability proof | 실제 Linux/React 빌드, worker 이벤트·암호 호환 시험; 비밀 없는 새 이력 |
| P1 영속 상태 | SQLite writer·tenant repository·계정/사용량 원장, journal, revision/epoch, run/effect/task 상태, 취소·lease·fence | 충돌·프로세스 종료·삭제·복원 fault injection 통과 |
| P2 대화 한 수직 흐름 | 다중 사용자 로그인·임시 비번·admin·provider 설정, 텍스트 대화, SSE, 재시도·취소·기록/삭제 | 제출→저장→실행→표시→재연결을 실제 Rust 경로로 완료 |
| P3 자료·기억·문서 | source ingest·검색·근거 읽기·기억·정정·망각·자동 추출·문서 | 오래된 기록·정정·삭제·범위 해제와 재시작 계약 통과 |
| P4 도구·학습·복합 작업 | tool registry/discovery, bounded harness, skills lifecycle·Git package 설치/제안 카드, 계획/검증/보완 | 도구 호출/결과 연결·최종 활성화·실패·예산·변경 시 무효화 통과 |
| P5 예약·소식·푸시 | 5초 due loop, occurrence, change-only, 알림/quiet hours, Push | 실제 시간·재시작·중복·정지·변경·기기 열기 검증 |
| P6 연결·외부 작업 | Google 읽기/초안/발송/일정, 공식 Gmail·Slack 수집, custom HTTP ingest·provenance, 공개 검색/배송 | 새 설치 OAuth, 연결/run/batch/source 추적·custom token 격리·cursor/partial·쓰기 영수증·결과 불명 계약 통과 |
| P7 첨부·모바일 완성 | 사진/파일/음성, 녹음, 미리보기, 압축, PWA 공유, 접근성 | 모바일/데스크톱·느린 네트워크·삭제 경합·실기기 검증 |
| P8 독립 볼트 | Rust 서버·React UI/격리 crypto Worker, CSV·TOTP·내보내기·복구·키 교체·credential broker | 기존 format 호환 벡터·취소/partial import·철회·독립 복원 통과 |
| P9 데이터 이전·제한 전환 | private importer, manifest, drain, 하나의 writer, 호환/복원 기록 | row/reference 대조, latest journal, 중복 effects0, 재시작/rollback drill |
| P10 안정판 출시 | 공개 개발 repo의 release, 문서, CI, SBOM, support, clean install/update/uninstall | 공개 허용 목록·권리·dependency·device·성능/품질 gate 모두 확인 |

공개 개발 저장소 준비는 P0에서 라이선스·허용목록·새 이력 gate를 충족한 뒤 가능하고 안정판 출시는 P10이다. 계획 검토가 GitHub 게시를 실행했다는 뜻은 아니다. P0에서 브라우저/crypto의 기술적 위험을 먼저 줄인다. P7·P8까지 그 가능성을 확인하지 않고 서버만 완성하지 않는다. 각 단계의 입력 파일·작업 순서·검증과 의존성은 [migration](migration.md)에 있다. 구현 중간에 실제로 쓸 수 있는 alpha는 P2부터 가능하지만 안정판 기능 완료와 구분한다.

## 반드시 가져갈 원칙

- **명령과 사실의 구분:** 사용자의 원래 요청만 작업 권한이다. 원문·메일·모델 출력·스킬이 새 권한을 만들지 않는다.
- **허용된 일은 바로 실행:** 조회·기억·문서 저장에 불필요한 승인 팝업을 붙이지 않는다. 명시된 범위의 외부 작업도 같은 권한을 다시 묻지 않는다. 실제로 모르는 수신인·대상·시각만 확인한다.
- **기억은 수정 가능한 사실:** 근거·화자·시각·유효기간·revision·confidence를 갖고 최신 정정을 우선한다. 망각과 원문 삭제의 범위를 구분한다.
- **한 요청의 수명:** request ID, run ID, attempt, checkpoint, local action receipt, effect ID를 분리해 같은 요청을 이어간다.
- **실행 기록과 완료의 구분:** 모델이 “완료”라고 말하는 것은 증거가 아니다. 실제 저장/외부 영수증과 postcondition을 검사한다.
- **회복 가능한 자동화:** 예정 회차는 DB가 정하고 모델은 그 일을 수행한다. 변경 없음·실패·판정 불명을 구별한다.
- **단순한 사용자 화면:** 데이터 건수·하네스 내부 상태·중복 확인을 기본 대화에 노출하지 않는다. 필요할 때 근거·상태·오류 이유를 연다.
- **최소 복잡성:** 비서 SQLite 한 개·독립 볼트 SQLite·DB별 하나의 writer·하나의 알림 경로를 우선한다. 큐·벡터 DB·멀티에이전트를 먼저 추가하지 않는다.
- **증거 있는 품질:** 자동 테스트, 합성 실제 모델 평가, 공개 live 표본, 실제 기기 시험을 따로 기록한다.

## 품질·속도 판단

과거 제품의 테스트 개수·모델 점수는 [이전 검증 기록](reviews/2026-11-12-initial-checks.md)에만 보존한다. 새 제품의 성적이나 실행 baseline으로 쓰지 않는다.

기존 업무집/테스트를 그대로 옮기지 않는다. 과거 결함은 참고하되 새 사용자 계약·위험·독립 oracle로 사례를 선정하고 중복·implementation mirror 검사는 버린다. [평가 계획](testing.md), [구체 예제](test-examples.md), [시나리오](../audit/evaluation-scenarios.tsv)가 새 기준이다. 새 구현에 맞춰 조정한 업무를 독립 평가라고 부르지 않는다. 새로운 선택 사례를 고정한 뒤 같은 model/effort·도구·예산·provider fixture에서 비교한다. 실패와 미실행을 분모에 남기고 사례 단위 신뢰구간을 낸다.

측정은 queue/context/model/tools/first-final-body/total과 스트림 표시 지연을 구분한다. 기본 모델이나 effort를 몰래 낮춰 빠르게 보이게 하지 않는다. 공개 모델 선택은 설치자가 바꿀 수 있고, 현재 사용자의 선택은 private importer가 보존한다. 기본 모델을 모든 계정이 지원한다고 가정하지 않는다.

paired95% 구간 하한>−3%p의 품질 non-inferiority 기준(표본/중단 규칙 사전 고정·불충분은 inconclusive), 부작용·삭제·볼트 계약 위반0, 하네스 자체 지연/메모리/CPU와 같은 작업당 token을 사전 기준으로 검사한다. 모델 지연 때문에 전체 p95가 개선되지 않으면 그대로 보고한다. 목표값과 실제 수치를 혼합하지 않는다.

## 이번에 정리하고 남길 것

새 폴더에는 설계·공개 기능·합성 검증 계약과 최소 Rust 도구만 둔다. 이전 source tree, vendor 실험, credential, 계정 선택 규칙, 실제 이메일 PDF, 운영 DB/backup/trace, 옛 `.git` 이력은 넣지 않는다. 개인용 connector는 공개 계약과 private 설정을 분리한다. 기존 운영 항목의 물리 삭제는 새 경로 검증·참조 확인·복원 수단 확보 후 단계별로 처리한다.

[audit](audit.md)의 모든 파일은 `rewrite`, `contract`, `reference`, `exclude`, `optional`, `replace` 중 하나로 결정한다. `exclude`는 새 제품 반입 제외이며 기존 운영 파일의 삭제 허가를 뜻하지 않는다. 기능 누락 여부는 [features](features.md), 파일 누락 여부는 `cargo run --locked -p xtask -- verify-plan`으로 확인한다.

## 함께 개발하고 사용하는 흐름

[CONTRIBUTING](../CONTRIBUTING.md)의 작은 PR과 [OpenSpec](../openspec/README.md) delta로 협업한다. typo/일반 recipe는 절차를 줄이고 새 동작·저장·권한·설치 계약에 집중한다. 공개 source는 GitHub, 사용자의 주 화면은 기존 제안/할 수 있는 일 카드다. 별도 marketplace를 기본 진입점으로 만들지 않는다.

계획된 [평가 하네스](../openspec/changes/build-agent-evaluation-harness/proposal.md), [community 스킬](../openspec/changes/add-community-skill-discovery/proposal.md), [credential 연결](../openspec/changes/connect-vault-credentials/proposal.md)은 active change로 두고 구현·시험 전 완료/archived로 표시하지 않는다. 이미 연결된 vault credential은 원래 사용자 요청의 범위에서 자동 사용하고 잠금·철회·모호함만 실제 상황대로 처리한다.

## 수집 구현 범위 갱신

R29에 따라 F38 카톡 수집·설치·전용 도구는 공개 제품 범위 밖이다. F70으로 공식/커스텀 공통
연결·상태·provenance를 구현하며 [data-connections](data-connections.md)와
[OpenSpec](../openspec/changes/build-source-connections/tasks.md)에 코드 좌표·순서·검증을 둔다.
공식 분류만으로 사용 가능을 주장하지 않으며, 실제 수집 runtime 완료는 별도 task로 남긴다.
