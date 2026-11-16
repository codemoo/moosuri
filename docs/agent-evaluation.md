# 합성 결과 평가 실행기

`moosuri-eval`은 실제 Rust HTTP router → Runtime → 새 SQLite 경로의 공개 회귀 사례를
실행한다. 별도 판정 코드가 저장 상태·도구 결과·모델 dispatch 영수증을 읽는다.
LLM 의미 이해 성적, 독립 holdout, 32세계 pilot, 이전 비서와의 비열등 평가가 아니다.
기존 테스트를 복사하지 않고 [평가 방법](testing.md)의 결과·실패 계약으로 구성했다.

```sh
cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json
cargo run --locked -p moosuri-eval -- --feature memory --report /absolute/new-memory.json
cargo test --locked -p moosuri-eval
```

`--report`를 생략하면 JSON을 stdout으로 출력한다. 파일은 새로만 생성하며 Unix에서는
0600으로 만든다. 기존 파일·symlink 대상은 덮어쓰지 않는다. 출력은 128KiB 이하이며
사례·판정기 검증 실패/미실행은 종료 코드 1, 잘못된 인자·출력 오류는 2다.
회귀 실행은 endpoint/model/credential/기존 DB/사용자 fixture 경로를 받지 않는다.
아래 별도 `baseline-pin` 명령은 과거 소스의 허용 목록만 읽으며 에이전트를 실행하지 않는다.
CI는 사용자 지시로 계속 꺼져 있으며 이 명령은 로컬 검증이다.

## 기능 선택과 형식 4

선택 인자는 맨 앞의 `--feature chat|memory|tasks|skills|schedules|monitors`다.
뒤에는 선택적으로 `--report /absolute/new.json`만 둘 수 있으며 `baseline-pin`이나
`paired-*`와 결합하지 않는다. 생략하면 기존 15세계와 필수 대조군을 모두 실행한다.
대화 3·기억 3·업무 4·스킬 3·예약 1·모니터링 1세계로 나누며 같은 사례를 중복 집계하지
않는다. 이 분류는 검증 범위 구분이고 제품 runtime의 routing 변경이 아니다.

`format_version: 4`는 행의 `feature`, `selection`의 선택/제외 사례 ID와 `full_suite`,
기능별 `features` 집계를 추가한다. 선택 내용도 profile digest에 포함된다. 미선택은
`not_selected`, 누락·실패·미실행 사례는 `fail`, 사례가 모두 통과해도 필요한 판정기
대조군이 없거나 실패하면 `unverified`다. 준비 실패를 분모에 남기는 공통 검사와
해당 기능의 필수 대조군은 선택 실행에서도 유지한다. 미선택 기능의 대조군을 생략한
결과를 전체 통과로 해석하지 않는다. 기존 completion/mission/handling 분모는 유지한다.

보고서의 `pass`는 고정 합성 세계의 계약 통과다. 실제 모델 이해력·provider·브라우저·
기기·장기 부하 수락과는 구분하며 [기능별 안정화 표](testing.md#기능별-안정화)를 따른다.

## 공개 세계와 판정

| 사례 | 실제 경로 | 독립 판정 |
| --- | --- | --- |
| 기억 수명 | 인증 HTTP 편집기로 직접 생성·정정·망각, 단계별 실제 검색/읽기, Store/Runtime 재개방 후 재질문 | 현재 사실·revision·읽기 영수증, tombstone과 version/FTS/파생 답변 purge, 망각 후 근거/이전 사실 없음 |
| 구독 종료·요청 replay | HTTP 접수 후 SSE body 종료, 같은 request UUID 재접수, 저장 대화 조회 | 같은 run·모델 dispatch 1회·완료 본문 보존 |
| 잘린 provider stream | 본문 뒤 terminal frame 없는 EOF | `outcome_unknown`, 완료 step 없음, dispatch 기록 보존; task 완료와 별도 handling 성공 |
| 저장 견적 계산 | 자신의 원문과 다른 사용자 방해 원문 적재, 실제 검색/원문 읽기/소수 계산 | 정확한 source ID/revision·입력·계산 영수증, 독립 정수 센트 정답, 최종 숫자/통화 사실 |
| 스킬을 이용한 계산 | 실제 metadata·정확한 본문 읽기와 위 계산 | 본문 읽기 외에 실제 원문/계산 결과 존재 |
| chat 기억 정정 | 채팅으로 기억→다른 thread 실제 검색/읽기→채팅 정정→재개방 후 회상 | committed 명령 outcome·모델 receipt, 같은 memory ID와 revision, 정정된 현재 사실 |
| 전이 망각 | memory 답변→실제 conversation_search/read 파생 답변→채팅 망각→재개방 | memory version/FTS·파생 출력/도구/checkpoint/재생 event purge, 새 회상에서 대상 없음; 같은 canary를 가진 독립 source와 답변 보존 |
| 실행 중 미션 취소 | provider 요청 도착 후 응답 barrier→owner cancel/replay→응답 release→재개방 | cancelled·grant 철회·uncertainty·dispatch accounting 보존, 늦은 achieved/새 dispatch 없음; handling으로만 집계 |
| 학습된 계산 재사용과 철회 | 다른 A/B 입력의 실제 decimal_workflow→unseen C learned_skill_execute→B thread 삭제→재개방→fresh D | 독립 scaled-integer 정답, candidate/usable·다른 finalized proof와 receipt hash, C의 pin/입력/내부 계산 영수증; proof 1개로 강등·C 출력 철회·D에 usable pin 없음 |
| durable 미션 | `/api/v1/missions` 명시 요청 → 실제 계획/읽기/계산/검증 → Store 재개방 | 미션 state/mode/plan/candidate, 영수증에 연결된 결과 근거와 achieved 상태 보존, 재dispatch 없음 |

도구 호출의 특정 순서는 oracle이 아니다. 발급 dispatch의 step/call ID·typed arguments와
SQLite 결과 slot을 양방향으로 연결하며 중복 영수증 재사용도 거부한다. 최종 근거와 함께
**검색에서 반환된 모든 후보의 ID/revision 소유권**을 검사한다. 다른 사용자 후보를 반환했다면
나중에 읽거나 인용하지 않아도 격리 실패다. 전체 문장과 비교하지 않고 fixture의 제한된
사실을 확인하며 일반 언어 이해·문체·의미 판정을 수행한다고 주장하지 않는다.

각 세계는 새 임시 디렉터리, 합성 계정·`.invalid` 주소·합성 자료를 사용한다. 외부 connector는
연결하지 않는다. 모델은 서버가 생성한 `127.0.0.1` ephemeral listener의 scripted compatible
protocol만 사용한다. capability 등록도 fixture 준비이며 실제 provider probe 증거가 아니다.
source 관측 시각만 고정하며 Runtime 전체 clock은 실제 시각을 사용한다. 아래 후속 모니터
시나리오의 due tick은 별도로 명시한 Store 합성 시각을 사용한다.
미션의 명시적 product acceptance check는 사용자 요청이고 evaluator의 숨은 정답 필드가 아니다.

## 이어지는 일상 업무

[일상 시나리오](everyday-scenarios.md)는 견적 검토 중 추가 지침, 빠진 기준을
질문한 뒤 재시작·답변으로 이어가기, 직전 알림의 시간 변경·재시도·취소를 추가한다.
형식3 보고서는 단계별 실제 모델/도구 호출과 한도, 결과 조건, 실패 후 미실행을 보존한다.
추가 지침 접수·대기·동일 요청 재전송·재개방은 모델0회를 요구한다. 본문·결과·영수증의
유지 여부를 판정하며, 준비한 답변만 맞추는 방식으로 통과시키지 않는다.

[환불 상태 후속 시나리오](followup-scenarios.md)는 자료 읽기·후속 확인 동의부터
변화 없는 점검, 갱신된 자료의 한 번 알림, 중지·재시작 뒤 알림 차단까지 이어진다.
HTTP·실제 Runtime/SQLite를 사용하며 scheduler의 공개 Store 메서드에 합성 due 시각을
전달한 구간은 보고서에 따로 남긴다. 알림의 실제 업무 링크와 저장 영수증·정확한 상태
인용을 함께 판정하고, 잘못된 링크나 모순된 지급 완료 문장이 있으면 실패한다.
후속 알림 평가의 인용 계약은 일반 자유문장 의미 평가와 구분한다.

[수리 견적 선택 시나리오](repair-choice-scenario.md)는 두 실제 원문을 읽은 뒤
모호한 선택의 거절, 올바른 선택·동일 요청 재전송, 재시작 뒤 기존 읽기 유지,
늦은 완료 응답보다 먼저 접수한 취소를 검증한다. 각 단계의 실제 호출 수와 원래
예산·대기 식별자를 보존하며, 취소된 작업을 완료율에 포함하지 않는다.

## 판정기 검증

주 제품 사례와 분리한 `validation_checks`는 정상 계산을 positive control로 사용하고 다음
변형을 거부한다: 완료 문장만 남기기, 계산 영수증 제거, 다른 call 영수증, 중복 영수증 재사용,
다른 소유자 근거, 읽지 않은 다른 소유자 검색 후보, 완료 단계 누락, 음수 금액,
미션 결과의 필수 원문 인용·원문 읽기/계산 영수증 누락. 실제 별도 실행에서도
스킬 본문만 읽고 계산 완료를 주장하는 provider를 거부한다. 이 음성 대조군은 task 성공이나
추가 표본으로 합산하지 않는다. 준비 실패가 planned/not-run 및 completion 분모에 남는지도
직접 검증한다. chat 정정의 다른 memory ID, 전이 망각의 남은 checkpoint,
취소 뒤 늦은 achieved, 중복 학습 proof, 내부 계산 영수증 누락과 철회 후 C 출력 잔존도
각 결과 oracle로 거부한다.

정답은 evaluator의 `grader.rs`에 있고 agent 요청 생성 모듈은 이를 import하지 않는다.
agent에는 사용자 발화·공개 스킬 metadata·실제 tool 결과만 전달된다. filesystem/shell이나
gold·관측 DB·upstream ledger를 읽는 도구는 없다. 공개 정답이므로 blind 평가라고 부르지 않는다.

## 보고서와 한계

모든 planned 사례의 executed/pass/fail/not-run을 보존한다. 설정 실패는 not-run, 실행 중
오류·timeout·postcondition 실패는 fail이다. `completion_eligible`/`task_success`는 실행 가능한
결과의 분모/성공이며, 실제 durable 미션은 별도의 `mission_eligible`/`mission_success`다.
잘린 stream과 명시적 owner 취소는 `handling_eligible`/`handling_success`로만 집계한다. 일반 chat continuity와
음성 대조군이 task 완료율을 높이지 않는다. 가족이 같은 두 계산·미션은 독립 세계 표본이 아니다.

`source_sha256`은 빌드 시 실제 Cargo manifest/lock, crates, migrations, community와 평가 도구의
소스 bytes를 정렬해 계산한다. dirty tree를 clean HEAD로 표시하지 않는다. fixture/profile/grader와
실제 요청 digest를 따로 기록하며(분리된 fixture/grader 하위 모듈도 digest에 포함), 원문 요청·응답·credential·DB는 보고서에 넣지 않는다.

시간은 monotonic HTTP 접수와 10ms polling terminal 관측이며 준비/workflow 시간을 분리한다.
관측 admission/terminal/workflow의 nearest-rank p50/p95와 sample 수를 제공하지만, 작은 공개
회귀 사례를 성능 부하 결과로 해석하지 않는다. 실패/timeout에도 관측한 workflow 시간·dispatch
수·DB usage를 보존한다. terminal usage가 없으면 token은 null이다. format_version 2는 접수된
각 run의 기존 `RunView.timings`를 별도 `server_timings` 배열로 보존한다. queue/context/최초 dispatch/
model/tool/최초 최종 본문/terminal의 실제 표본·누락 수와 측정 model/tool 호출 분모를 함께 보고한다.
진행 중 오류로 workflow 결과를 못 얻어도 이미 접수한 run은 누락 표본으로 남는다. 내부 미션은
RunView API 표본을 수집하지 않아 누락으로 표시한다. 가격·DOM·RSS/CPU는 null이며 polling 지연은
TTFT가 아니다. 시간 값·누락 여부는 기존 결과·예산 판정을 바꾸지 않는다.
기존 run의 6 model/12 tool 한도, provider 세계당32회, 준비/실행 각각40초를 적용하고 shutdown은
기존 Runtime의 bounded drain을 따른다.

실행 가능한 legacy baseline/공통 adapter, 실제 model profile, calibrated judge/사람 검토,
통계 비교·holdout·부하·실계정/기기와 더 넓은 SKILL02~05 수락은 미완료다. 위의 한 가지 실제
compound A/B→C·proof 철회 경로는 일반 학습 품질이나 no-skill/discovery/learned 비용 비교를
입증하지 않는다. 현재 reopen은 정상 종료 후 재개방이며 crash/kill checkpoint-gap world와 실제
provider 수락·지연 profile은 별도 미완료 범위다.
[현재 상태](status.md)와 [active change](../openspec/changes/build-agent-evaluation-harness/tasks.md)가
전체 수락 범위의 정본이다.

[2026-11-15 실모델 개발 평가](reviews/2026-11-15-live-agent-quality.md)는
GPT-6.1 Terra/high의36개 실모델 분류·5개 무호출 분기와 실제 앱11개 연속 시나리오,
스킬 재사용·위임/예약 확장을 별도로 기록한다. 임시 공식 이력 재입력 연결을 사용했으며
원래 실패와 채점기 오판, 의미 검토, 후속 수정 결과를 구분한다. 이 작은 개발 평가를
위32세계×3반복 pilot·독립 holdout·운영 provider 수락으로 대체하지 않는다.

[2차 실모델 평가](reviews/2026-11-15-live-agent-wave2.md)는 입력/정답을 사전 고정한
새8세계의22개 요청을 독립 판정했다. 최초 의미11통과·7실패·4미실행/엄격8통과·
10실패·4미실행과, 수정 후 새 DB에서의 전체22/22통과를 구별한다. 정확한 소수 표시의
학습 근거 채택, 분류기 간 실행 단계, 소유자를 명시한 기억 대상 문제를 수정했다.
별도 실제 예약에서는 학습 실행→철회로 예정 회차 정지→명시적 재개 후 새 실행을
확인했다. 최초 실패와 후속 결과를 합산하지 않으며, 수정에 사용한 사례의 재실행은
holdout으로 세지 않는다. 임시 연결의 지연은 선형 보간 quantile로 별도 표기하며
위 공개 Rust 실행기의 nearest-rank 집계 및 운영 provider 속도와 혼용하지 않는다.

## 기존 비서의 소스 기준 고정

### 두 공통 세계의 로컬 실행 adapter

`paired-prepare`는 기존149개 허용 목록만 새0700 Mac capsule로 복사·재검증하고,
고정 lock의 공개 의존성만 별도 준비한다. 원본에서 실행하지 않으며 옛 테스트·설정·DB는
복사하지 않는다. 현재 adapter는 Homebrew PostgreSQL16와 Python3.13, macOS Seatbelt가
필요하다. 다른 OS나 격리 실패는 미실행으로 남는다. 공개 제품·orchestration·fixture·grader는
Rust이며 Python은 변경하지 않은 private benchmark subject뿐이다.

```sh
moosuri-eval paired-prepare --source /absolute/legacy-root \
  --capsule /private/tmp/moosuri-paired-subject-UNIQUE --uv /absolute/uv
moosuri-eval paired-baseline --capsule /private/tmp/moosuri-paired-subject-UNIQUE \
  --report /absolute/new-paired-report.json
```

실행 입구는 원본 source pin만으로 열리지 않는다. canonical `/private/tmp`의 직접 자식인
`moosuri-paired-subject-*`, 현재 UID·0700·symlink 없는 경로, 허용 목록/manifest에 묶인
새 copy marker와 준비 완료 marker, 고정 interpreter·전체 venv의 runtime content pin을
함께 검사한다. 두 marker는 새 복사·dependency 준비 절차에서만 생성한다. 예전 marker 없는
capsule이나 원본 checkout을 자동 승인하지 않는다. `paired-check --capsule ... --report ...`는
동일한 검사를 child process 없이 실행한다. 같은 UID의 악의적 관리자를 인증하는 서명 체계는 아니다.

두 번째 명령은 capsule 재검증 후 세계마다 새 private PG16 cluster와 API/runner를 시작한다.
PG는 private Unix socket만 열고 TCP를 사용하지 않는다. 비어 있는 child 환경에 합성 설정과
loopback 모델만 주며 background/effects/collector는 시작하지 않는다. Seatbelt는 원래 사용자
tree와 다른 tmp/volume의 내용 읽기, task state 밖 쓰기, 임의 network와 mach lookup을 막는다.
`initdb`의 상위 경로 확인에 필요한 `/private/tmp`와 해당 run root의 정확한 metadata만
허용하며, macOS 유효 사용자 ID 조회용 `com.apple.system.opendirectoryd.libinfo` 하나만
Mach 예외로 둔다. 내용 읽기나 다른 Mach 서비스 허용으로 확장하지 않는다.
새 canary 읽기/쓰기 거부·허용 경로 positive control·허용하지 않은 loopback port 거부를
실제로 확인한 뒤에만 subject를 시작한다. 실행 권한 때문에 child Seatbelt가 불가능한
상위 sandbox 안에서는 우회하지 않고 `not_run`으로 끝낸다. 기존 service/cron/cluster를
시작·변경하지 않는다. 소유한 process group은 종료·회수한 뒤 임시 state를 지운다.

공통 범위는 (1) 직접 입력한 timeless 기억의 조회와 정상 종료/재개방 후 조회,
(2) 현재 사용자 입력의 두 금액 합계뿐이다. 각각 새 DB를 사용한다. legacy Responses와
현재 compatible-chat fixture는 실제 전달된 문맥/도구 결과/사용자 금액에서 답을 만든다.
독립 grader가 저장 기억·현재 revision·두 read evidence·최종 사실과 정수 cents를 확인하며,
성공 문장만 있거나 read evidence가 없거나 금액이 틀린 대조군을 거부한다. 옛 실행기에
없는 calculator receipt나 다중 사용자 격리를 공통 성공 조건으로 요구하지 않는다.

보고서는 두 세계의 planned/executed/pass/fail/not_run, 고정 오류·체크·digest만 담고
입출력 본문·host path·설정·secret은 담지 않는다. 일반 실행과 같이 새0600 파일만 만든다.
backend별 attempted·실제 HTTP admission의 executed·관측 결과 반환 여부를 분리하며,
접수 이후 observer 실패/timeout도 executed/fail 분모에 남긴다.
이것은 deterministic protocol/state 비교이며 실제 모델 이해력·같은 모델 성능·비열등성·
32세계 pilot·시간 사실·abrupt crash·실제 provider 시험을 완료한다는 뜻이 아니다.
현재 실제 비교는2건 모두 미실행이다. 초기 시작 권한 오류를 고쳤으나 시험 호스트의
System V 공유 메모리32개 슬롯이 이미 점유되어 PG bootstrap이 막혔다.
기존 자원을 삭제하거나 전역 설정을 바꾸지 않았다. 실행기 구현·합성 대조군 검증과
실제 비교의 수락을 구분하며 [단위 기록](reviews/2026-11-15-paired-baseline.md)을 따른다.

첫 안정판의 비교 대상은 기존 비서다. 기존 Git HEAD만으로 추적되지 않는 runtime을
고정했다고 주장하지 않고, 계획 당시 `audit/source-manifest.tsv`의 소스 bytes를 사용한다.
이 자료는 일반 설치와 공개 회귀 실행에 필요하지 않다.

```sh
cargo run --locked -p moosuri-eval -- baseline-pin \
  --source /absolute/legacy-root --report /absolute/new-pin-report.json
```

고정된 manifest 해시에서 `agent/src/musuri_agent`, `agent/migrations`, 세 dependency/lock
파일만 선택한다. 총149개/1,750,513byte가 모두 맞아야 성공하며 누락·변조·심볼릭 링크·
추가 runtime 파일을 거부한다. Python bytecode cache는 제외하며 이후 실제 adapter는
반드시 새 허용 목록 복사본에서 실행해야 한다. 원본 tree·Git·DB·서비스는 수정하지 않는다.
소스를 import하거나 실행하지 않고 `.env`, 개인 설정, credential, DB, 기존 평가 trace와
과거 테스트는 읽지 않는다. 보고서에는 고정 hash·수·검사 결과만 담고 경로/원문을 넣지 않는다.

이 검사는 **내용 일치 사전 검사**다. 실행할 소스를 복사/봉인하지 않으며 이후 변경을
막지도 않는다. 실제 비교 실행은 격리한 소스의 재검증, 고정 provider/model/effort·prompt,
공통 fixture adapter와 별도 PostgreSQL을 필요로 한다. 현재 report의 runtime/adapter/
model profile/non-inferiority는 모두 `not_run`이다. 이전 실행기는 SQLite가 아니며
Python과 PostgreSQL의 실제 시간을 함께 사용하므로 공통 fake clock도 아직 없다.
정확 계산 도구가 없는 이전 실행기에 새 calculator receipt를 요구해서는 안 된다.
초기 공통 범위는 시간에 무관한 기억 조회/사용자 입력 계산부터 별도로 고정해야 한다.


## 제품 경로의 서버 시간과 평가 보고서

현재 제품의 run/usage API는 [서버 실행 시간](run-timings.md)을 숫자로 관측한다. 원래
attempt·모델/effort·예산을 바꾸지 않으며 실제 수집된 queue/context/model/tool/final-body/
terminal 표본만 제공한다. 보고서 format2는 현재 실행의 RunView 표본을 함께 집계하며,
HTTP/polling 시간과 서버 내부 시간을 구분한다. 과거 보고서에 소급 복사하거나 작은
회귀 결과를 provider/기기 성능 표본으로 합치지 않는다. phase 부재·실패·재시작·직접 SQL 전이의
누락과 원래 outcome 분모를 유지한다. 실제 성능 비교는 사전에 고정한 부하/표본 계획과
서버·브라우저 각각의 관측이 추가로 필요하다.

[합성 Linux 결과와 평가 중 수정한 결함](reviews/2026-11-15-agent-outcome-workflows.md).
