# 합성 결과 평가 실행기

`moosuri-eval`은 실제 Rust HTTP router → Runtime → 새 SQLite 경로의 공개 회귀 사례를
실행한다. 별도 판정 코드가 저장 상태·도구 결과·모델 dispatch 영수증을 읽는다.
LLM 의미 이해 성적, 독립 holdout, 32세계 pilot, 이전 비서와의 비열등 평가가 아니다.
기존 테스트를 복사하지 않고 [평가 방법](testing.md)의 결과·실패 계약으로 구성했다.

```sh
cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json
cargo test --locked -p moosuri-eval
```

`--report`를 생략하면 JSON을 stdout으로 출력한다. 파일은 새로만 생성하며 Unix에서는
0600으로 만든다. 기존 파일·symlink 대상은 덮어쓰지 않는다. 출력은 128KiB 이하이며
사례·판정기 검증 실패/미실행은 종료 코드 1, 잘못된 인자·출력 오류는 2다.
회귀 실행은 endpoint/model/credential/기존 DB/사용자 fixture 경로를 받지 않는다.
아래 별도 `baseline-pin` 명령은 과거 소스의 허용 목록만 읽으며 에이전트를 실행하지 않는다.
CI는 사용자 지시로 계속 꺼져 있으며 이 명령은 로컬 검증이다.

## 공개 세계와 판정

| 사례 | 실제 경로 | 독립 판정 |
| --- | --- | --- |
| 기억 수명 | 인증 HTTP 편집기로 직접 생성·정정·망각, 단계별 실제 검색/읽기, Store/Runtime 재개방 후 재질문 | 현재 사실·revision·읽기 영수증, tombstone과 version/FTS/파생 답변 purge, 망각 후 근거/이전 사실 없음 |
| 구독 종료·요청 replay | HTTP 접수 후 SSE body 종료, 같은 request UUID 재접수, 저장 대화 조회 | 같은 run·모델 dispatch 1회·완료 본문 보존 |
| 잘린 provider stream | 본문 뒤 terminal frame 없는 EOF | `outcome_unknown`, 완료 step 없음, dispatch 기록 보존; task 완료와 별도 handling 성공 |
| 저장 견적 계산 | 자신의 원문과 다른 사용자 방해 원문 적재, 실제 검색/원문 읽기/소수 계산 | 정확한 source ID/revision·입력·계산 영수증, 독립 정수 센트 정답, 최종 숫자/통화 사실 |
| 스킬을 이용한 계산 | 실제 metadata·정확한 본문 읽기와 위 계산 | 본문 읽기 외에 실제 원문/계산 결과 존재 |
| durable 미션 | `/api/v1/missions` 명시 요청 → 실제 계획/읽기/계산/검증 → Store 재개방 | 미션 state/mode/plan/candidate, 영수증에 연결된 결과 근거와 achieved 상태 보존, 재dispatch 없음 |

도구 호출의 특정 순서는 oracle이 아니다. 발급 dispatch의 step/call ID·typed arguments와
SQLite 결과 slot을 양방향으로 연결하며 중복 영수증 재사용도 거부한다. 최종 근거와 함께
**검색에서 반환된 모든 후보의 ID/revision 소유권**을 검사한다. 다른 사용자 후보를 반환했다면
나중에 읽거나 인용하지 않아도 격리 실패다. 전체 문장과 비교하지 않고 fixture의 제한된
사실을 확인하며 일반 언어 이해·문체·의미 판정을 수행한다고 주장하지 않는다.

각 세계는 새 임시 디렉터리, 합성 계정·`.invalid` 주소·합성 자료를 사용한다. 외부 connector는
연결하지 않는다. 모델은 서버가 생성한 `127.0.0.1` ephemeral listener의 scripted compatible
protocol만 사용한다. capability 등록도 fixture 준비이며 실제 provider probe 증거가 아니다.
source 관측 시각만 고정하며 Runtime 전체 clock은 실제 시각을 사용한다.
미션의 명시적 product acceptance check는 사용자 요청이고 evaluator의 숨은 정답 필드가 아니다.

## 판정기 검증

주 제품 사례와 분리한 `validation_checks`는 정상 계산을 positive control로 사용하고 다음
변형을 거부한다: 완료 문장만 남기기, 계산 영수증 제거, 다른 call 영수증, 중복 영수증 재사용,
다른 소유자 근거, 읽지 않은 다른 소유자 검색 후보, 완료 단계 누락, 음수 금액,
미션 결과의 필수 원문 인용·원문 읽기/계산 영수증 누락. 실제 별도 실행에서도
스킬 본문만 읽고 계산 완료를 주장하는 provider를 거부한다. 이 음성 대조군은 task 성공이나
추가 표본으로 합산하지 않는다. 준비 실패가 planned/not-run 및 completion 분모에 남는지도
직접 검증한다.

정답은 evaluator의 `grader.rs`에 있고 agent 요청 생성 모듈은 이를 import하지 않는다.
agent에는 사용자 발화·공개 스킬 metadata·실제 tool 결과만 전달된다. filesystem/shell이나
gold·관측 DB·upstream ledger를 읽는 도구는 없다. 공개 정답이므로 blind 평가라고 부르지 않는다.

## 보고서와 한계

모든 planned 사례의 executed/pass/fail/not-run을 보존한다. 설정 실패는 not-run, 실행 중
오류·timeout·postcondition 실패는 fail이다. `completion_eligible`/`task_success`는 실행 가능한
결과의 분모/성공이며, 실제 durable 미션은 별도의 `mission_eligible`/`mission_success`다.
잘린 stream은 `handling_eligible`/`handling_success`로만 집계한다. 일반 chat continuity와
음성 대조군이 task 완료율을 높이지 않는다. 가족이 같은 두 계산·미션은 독립 세계 표본이 아니다.

`source_sha256`은 빌드 시 실제 Cargo manifest/lock, crates, migrations, community와 평가 도구의
소스 bytes를 정렬해 계산한다. dirty tree를 clean HEAD로 표시하지 않는다. fixture/profile/grader와
실제 요청 digest를 따로 기록하며 원문 요청·응답·credential·DB는 보고서에 넣지 않는다.

시간은 monotonic HTTP 접수와 10ms polling terminal 관측이며 준비/workflow 시간을 분리한다.
관측 admission/terminal/workflow의 nearest-rank p50/p95와 sample 수를 제공하지만, 작은 공개
회귀 사례를 성능 부하 결과로 해석하지 않는다. 실패/timeout에도 관측한 workflow 시간·dispatch
수·DB usage를 보존한다. terminal usage가 없으면 token은 null이다. 가격·queue/context·
first-final-body·DOM·RSS/CPU는 측정하지 않아 null이다. polling 지연은 TTFT가 아니다.
기존 run의 6 model/12 tool 한도, provider 세계당32회, 준비/실행 각각40초를 적용하고 shutdown은
기존 Runtime의 bounded drain을 따른다.

실행 가능한 legacy baseline/공통 adapter, 실제 model profile, calibrated judge/사람 검토,
통계 비교·holdout·부하·실계정/기기와 SKILL02~05의 실제 학습 proof는 미완료다.
[현재 상태](status.md)와 [active change](../openspec/changes/build-agent-evaluation-harness/tasks.md)가
전체 수락 범위의 정본이다.

## 기존 비서의 소스 기준 고정

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
