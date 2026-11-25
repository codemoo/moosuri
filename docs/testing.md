# 테스트·비서 능력 평가 계획

이 문서는 검증 방법과 전체 출시 평가 계획이다. 실제 Rust/React 계약 시험과 제한된 모델 관측은 [현재 상태](status.md)의 소스별 기록을 따른다. [합성 결과 실행기](agent-evaluation.md)는 실제 HTTP/Runtime/SQLite의 공개 회귀 세계와 독립 판정을 제공한다. 아래 전체 모델 평가·32세계 pilot·holdout·baseline 비열등 비교를 완료했다는 뜻은 아니다. `xtask verify-plan`은 문서/추적 표만 검사한다. 기존 Python772/TS215 선언은 전수조사의 역사적 수이며 새 테스트 목표가 아니다. 기존 테스트를 그대로 옮기지 않는다.

## 기능별 시나리오 실행 기록

이 설치의 대화·기억 시나리오에서 실제 추론은 Claude Sonnet 5.5(`claude-sonnet-5-5`, `medium`)만 사용한다. 대화는 회원 웹 실행기의 `--claude`, 장기 기억은 `python3 audit/memory-reader/run.py`로 실행한다. 두 실행기는 공식 Claude CLI의 현재 로그인을 확인하고 매 답변의 사용량에 기록된 모델 ID를 검사한다. 인증이나 지정 모델을 확인할 수 없으면 실행을 중단한다. [대화 시나리오와 명령](reviews/2026-11-20-kiri-scenarios.md#claude-대화-검증) · [장기 기억 시나리오](../audit/memory-reader/results.md)

[기능 대장](../audit/feature-journeys.tsv)은 F01–F71의 각 시나리오에 조작, 기대 결과, 증거 파일·테스트 이름, 실행 상태를 기록한다. 같은 테스트가 두 사용자 결과를 확인하는 경우 같은 근거를 연결한다. `통과`는 명시한 환경에서 기대 결과를 관찰한 행이다. `부분 확인`과 `실환경 대기`는 실제 계정·기기·플랫폼 조건이 남은 행이고, `후속 범위`는 제품에서 후속으로 정한 기능이다. [회원 웹 시나리오](reviews/2026-11-20-kiri-scenarios.md) · [대화와 업무 인계 27개 시나리오](reviews/2026-11-25-conversation-work.md) · [표준 서버 실행 기록](../audit/standard-journey-checks.md)

자동 실행 결과를 대조할 때 다음 명령에 각 테스트 실행의 실제 보고서를 넘긴다. Rust 로그를 여러 개 넣을 때는 수정 후 다시 실행한 결과를 뒤에 둔다. 파일명·테스트 이름 또는 실제 실행 결과가 빠진 행은 미확인으로 반환한다. 실행한 절차·장비·모델의 관측은 각 행의 증거 문서에 기록한다.

```sh
python3 audit/verify-feature-journeys.py --vitest-json web-results.json --vitest-json vault-results.json --rust-log workspace-tests.log --rust-log focused-reruns.log
```

채팅에서 맡기는 업무는 일반 입력창과 같은 회원의 원래 요청, SQLite 회차, 읽은 출처 또는 공급자가 돌려준 ID를 함께 확인한다. 정상 요청·다른 표현·후속 수정·재전송·철회·회원 간 격리를 기능당 3~10개로 묶는다. 실제 사용에서 관찰한 표현은 합성 자료로 재구성한다. [업무별 시나리오와 결과](reviews/2026-11-25-member-work-from-chat.md) · [실제 Claude 회원 화면 25개 사례](reviews/2026-11-25-live-member-journeys.md) · [입력창·감시·재시도·대화 전환·선호 회상 32개 사례](reviews/2026-11-25-composer-routing-and-recovery.md) · [장보기·시간표·가격 알림 실사용 시나리오](reviews/2026-11-25-everyday-muse.md) · [Linux 실행 방법](../deployment/kiri/linux-vm-smoke/README.md#chat-work)

## 기능별 안정화

기능별로 **중복 책임 통합·불필요한 경로 제거·기존 동작 검증**을 함께 진행한다.
기존 사용자 흐름의 정상 결과·실패·재시도·취소·재시작을 묶어 확인한다.
아래 여섯 묶음은 기존 공개 15세계를 나눈 것으로, 새 표본이나 전체 제품
인증이 아니다. 재현한 결함은 해당 흐름에 회귀를 남기고, 변경하지 않은 검사를 매번
반복하지 않는다. 실행 결과는 [이번 안정화 기록](reviews/2026-11-16-feature-stability.md)과
[현재 상태](status.md)에 남긴다.

| 기능 / 요구 | 지금 확인하는 계약 | 별도로 남은 수락 |
| --- | --- | --- |
| 대화 `chat` / F03–08 | 접수 재전송·연결 종료·잘린 응답·작업 중 추가 지시 | 실제 모델 지연, 모바일 복귀·장기 연결 |
| 기억 `memory` / F19–22 | 생성·정정·재개방·전이 망각, 독립 자료 보존 | 현재 후보 선택 경로의 실모델 정정·망각 정확도; 이전 7/10을 새 경로 성적으로 바꾸지 않음 |
| 업무 `tasks` / F27 | 실제 결과·영수증, 보완 답변·재시작·늦은 응답보다 먼저 접수한 취소 | 여러 실제 업무의 의미 이해·미지원 요청 처리 |
| 스킬 `skills` / F24–26 | 원문 계산·실제 도구 사용·A/B 학습 뒤 새 C 재사용·철회 | 여러 실제 모델/스킬의 선택 정확도, community 설치·업데이트 사용자 흐름 |
| 예약 `schedules` / F29–30 | 직전 알림 수정·취소; 별도 Store 회귀로 같은 예약의 미완료 1회차·최신 대기 1회차 | 긴 중단·부하·실제 시간대/기기 알림 수신 |
| 모니터링 `monitors` / F28, F31–32 | 한정한 동의·무변화 모델 0회·변경 알림·중지·재시작 | 실제 연결의 자료 지연·철회·알림 수신 |
| 메일·일정·브라우저 / F33–37, F42 | 기존 합성 영수증·최종 승인·결과 불명 재발송 금지 기록 유지 | 위 15세계 밖; 실제 provider 연결·만료·재시도 수락 |
| 수집·검색 / F39–41, F70 | 기존 사용자별 원문·cursor·철회 계약 기록 유지 | 위 15세계 밖; 공식/커스텀 연결의 중단·복구·중복 동기화 |
| 이미지·녹음·PWA·푸시 | 기존 업로드·초안·전송·권한 검증 기록 유지 | 위 15세계 밖; 실제 기기에서 업로드 실패·복귀·오프라인·푸시 |
| 계정·모델 연결·설치 / F53, F56–64 | 기존 세션·계정 격리·연결/복구 검증 기록 유지 | 위 15세계 밖; 새 설치·다중 사용자·provider 만료의 실제 환경 |
| 볼트·broker·백업 / F44–52, F69 | 기존 암호·credential·삭제 journal·복원 경계 기록 유지 | 위 15세계 밖; 배포 패키지·신뢰 기기·재해 복구 수락 |

구조 정리도 같은 기능 단위로 수행하며 다음 책임을 확인한다.

| 통폐합 대상 | 보존할 차이와 완료 조건 |
| --- | --- |
| 실행·대기·재개·취소 | 같은 전이는 공통 처리; 원래 요청/예산/lease·이미 완료한 도구·늦은 결과 차단 유지 |
| 저장·재시도·결과 처리 | 같은 로컬 저장 정책은 공통 처리; 결과 불명 재쓰기·외부 작업 재전송 금지 |
| 기억·문서·도구 조율 | 공통 실행/오류 처리는 통합; 대상 선택·revision·근거·도메인 권한은 해당 책임에 유지 |
| 수집·연결·화면 상태 | 중복 접수/오래된 응답 처리는 공통화 후보; 계정/연결별 격리와 provider 차이는 보존 |
| 대체된 구현·미사용 코드 | 참조·동적 등록·migration/recovery 사용 여부 확인 후 제거; 호환 경로를 추측으로 삭제하지 않음 |

파일 수·삭제 줄 수·검사 개수가 성과 기준은 아니다. 같은 규칙을 수정할 장소가 줄고,
영향받는 실제 사용자 흐름의 기존 결과와 실패 경계가 유지되어야 완료다.
[공통 Store 재시도](reviews/2026-11-16-store-retry-consolidation.md)는 첫 적용으로,
실행기·메일의 7개 반복 루프를 하나로 합치면서 로컬 저장과 실행 권한 검사의 오류
정책을 구분한다. 앞선 공통 planner·미션 round·대기 재개 구현도 계속 사용한다.
[모델 응답 저장](reviews/2026-11-16-round-checkpoints.md)은 채팅·업무·분류기·Claude의
저장 4곳을 같은 정책으로 모은다. 실제 Store 대기열 포화 뒤 저장·재시작·도구 완료와
재시도 전 취소를 확인하며, 모델/외부 작업 재전송과 결과 불명 재쓰기를 허용하지 않는다.
[지침·답변 완료 처리](reviews/2026-11-16-answer-settlement.md)는 지침 상태와
완료/다음 응답 전환을 같은 트랜잭션으로 묶는다. 뒤쪽 저장 실패 때 전체 표시 상태가
되돌아가고, 실패한 표시 변경이 후속 모델 호출을 허용하지 않는지 실제 SQLite에서
검사한다. 공통 종료 모듈로 옮긴 기존 취소·권한·업무 검증 경계도 보존한다.
[업무 자리 정책](reviews/2026-11-16-mission-capacity.md)은 연결 대기 모듈에 있던
공통 제한과 각 대기/재개 경로의 중복 질의를 mission 책임으로 모았다. 전체 16개가
차도 기존 업무의 대기·답변·재개는 가능하고 새 업무만 거절하는지 실제 Store 흐름으로
확인한다. 원래 업무의 예산·프로필, 다른 계정의 독립성, 일시정지 자리와 접수 순서를
보존한다. 서로 다른 authority/복구 규칙을 단순히 같은 SQL이라는 이유로 합치지 않는다.

우선순위는 데이터 손실·중복 실행 방지 → 기억/대화/업무의 실제 의미 정확도 → 외부
연결 복구 → 기기 경험이다. 저장 계약이 통과해도 모델 정확도나 기기 경험이 검증된
것으로 표시하지 않는다. 각 단계는 영향을 받는 기존 검사와 대표 사용자 흐름으로
끝내고, 구체적 실패·새 변경이 없으면 같은 작은 검사를 반복하지 않는다.

```sh
cargo run --locked -p moosuri-eval -- --feature memory --report /absolute/new-memory.json
```

`chat|memory|tasks|skills|schedules|monitors` 중 하나를 선택한다. 전체 실행은
`--feature`를 생략한다. 선택하지 않은 기능은 `not_selected`이며 통과가 아니다.
[실행기 형식과 범위](agent-evaluation.md)를 따른다. CI는 계속 꺼둔다.

## 무엇을 남기고 무엇을 버릴지

테스트는 **사용자에게 중요한 동작·발생 가능한 장애·실패를 판정할 독립 근거**가 있을 때만 만든다. 각 [시나리오](../audit/evaluation-scenarios.tsv)는 목적, 입력, 기대 결과, 실패 주입, 측정치, 관련 기능을 갖는다. 한 행은 시나리오 묶음이며 테스트 함수 수를 뜻하지 않는다. 표의 planned는 전체 묶음의 수락 전 상태다. 부분 구현의 실제 증거는 현재 상태와 해당 검증 기록에서 구분한다.

| 기존 테스트 유형 | 새 계획의 결정 |
| --- | --- |
| framework/import/private helper·mock 호출 순서·내부 dict shape만 확인 | 제외. 새 Rust/React 구조의 요구가 아님 |
| 특정 prompt 문구·중간 reasoning·정답의 고정 표현을 비교 | 제외. 사실·조건·근거·결과 상태로 새 oracle 작성 |
| 같은 CRUD/클릭을 unit/API/browser마다 반복 | 가장 싸고 확실한 계층에 통합. 사용자 흐름 E2E는 대표 경로만 |
| 폐기할 엔진·hosting·개인 설정·모델 출력 복사 | 제외. 새 제품에 존재하는 위험으로 정당화되지 않으면 대체도 하지 않음 |
| 이미 겪은 데이터 손실·중복 발송·삭제 부활·세션 혼선 | 사고 원인과 사용자 계약만 가져와 새 경로로 재현 |
| 지나치게 넓은 screenshot/통계·시간대 의존 flaky test | layout/접근성/상태 assertion으로 줄이고 실제 시각 검증은 대표 기기로 분리 |
| 기존 합성 fixture | 기본 재사용 안 함. 권리·누출·oracle·현재 요구가 검증된 입력만 선택적으로 참고 |

기존112개 test/support 파일은 참고 또는 제외 대상으로 바꾼다. [기존 시험 표면](../audit/test-surfaces.tsv)과 source inventory의 `test_contract`는 역사적 근거 위치이며 이관 체크리스트가 아니다. 신규 테스트 PR은 “이 테스트가 없으면 통과할 실제 결함”을 한 문장으로 설명한다. 의미 없는 line coverage·테스트 개수 목표를 두지 않는다. 같은 실패를 충분히 잡는 검사가 있으면 새 검사를 추가하지 않고 병합한다.

## 검증 계층과 실행 비용

| 계층 | 검증 대상 | 실행 환경·주기 |
| --- | --- | --- |
| contract | ID/시간/권한/상태 전이, 실제 SQLite repository·가짜 provider·실제 Rust runner | 변경 관련 PR. 합성 자료, network off, fake clock. 문자열 함수마다 test를 만들지 않음 |
| fault | 저장/발송/체크포인트 경계의 kill·lost ACK·late response·복원 | 해당 경계 변경 PR·출시. 실제 local filesystem SQLite/WAL·별도 프로세스 |
| model | 실제 LLM이 요구를 이해하고 tool을 골라 완수하는지 | 작은 선택 평가 또는 출시 평가. 실제 Rust 전체 경로+합성 환경, 토큰/금액 상한 |
| ui | 채팅/카드/첨부/기억 관리의 실제 사용자 행동 | React component로 싼 검사, 대표 Playwright E2E만. mock UI 통과와 실제 API 통합 구별 |
| load | 동시 사용자·큰 자료·queue·latency·메모리 | 관련 성능 변경·출시. hardware/provider/자료 크기/worker 수 고정 |
| device | 마이크·키보드·Push·공유·설치·safe area | 실제 iOS Safari/PWA·Android Chrome·desktop, 지원 버전/기기 기록 |
| live | OAuth·provider 실계정·외부 쓰기 | 전용 시험 계정/수신처·명시된 시험 범위에서만. fork CI와 production credential 사용 금지 |

deterministic fake-model 시험만으로 모델 이해 능력을 증명하지 않는다. 실제 LLM 단독 Q&A만으로 저장·재개·외부 작업 능력을 증명하지도 않는다. 모델 시험은 API admission→runner→SQLite/connector fixture→결과 발행까지 연결한다. 지원하지 않는 local model의 도구 능력은 unavailable로 표시하며 full-assistant 성적으로 합치지 않는다.

## 합성 세계와 정답 구성

fixture는 `case_id, family_id, world_id, split, seed, clock/timezone, users, initial_state, source_events, user_turns, capability_profile, fault_points, expected_outcome, required_postconditions, forbidden_effects, evidence_ids, budgets, rubric_version`을 가진다. 실행 설정은 source revision·provider/model/effort·도구/스킬/catalog 버전·prompt digest·평가자 버전·하드웨어를 별도로 고정한다.

정답과 판정 코드는 agent의 prompt/tool/filesystem에서 접근할 수 없는 evaluator 영역에 둔다. 사용자 simulator에는 그 사용자가 실제 아는 사실과 예정한 후속 발화만 준다. 정답·숨은 provider 상태를 알려주지 않는다. agent가 수정한 own output을 정답으로 재사용하지 않는다. 기본 simulator는 deterministic script; 다양한 자연어/대화 분기는 별도 평가다. simulator/judge의 실패도 기록한다.

합성 이름·`.invalid` 주소·가짜 운송장·정해진 시간만 사용한다. 실제 카톡/메일/볼트/과거 trace를 익명화했다고 자동 공개하지 않는다. 스킬 공유 fixture 역시 private trace를 복사해 지우는 방식 대신 typed route와 합성 예제로 새로 만든다. 사례별 source/license/provenance를 기록한다.

같은 세계의 이름/날짜만 바꾼 paraphrase와 동일 절차 파생은 같은 family로 묶어 train/dev/holdout 사이에 새지 않게 한다. 평가 세트를 보고 tuning했다면 regression으로 이동한다. 공개 fixture는 재현용 regression이며 독립 blind holdout이라고 부르지 않는다. 출시 비교용 새 holdout은 implementation 작성자와 분리해 고정하고 평가 뒤 공개 가능 여부를 검토한다. secret gold를 repository에 넣고 “보지 않았다”고 주장하지 않는다.

## 메모리 평가

쓰기/추출→검색→답변→정정/망각→재시작을 각각 관측한다. 메모리 검색 결과만 정답이어도 최종 답변이 틀리면 사용자 성공은 실패다. 답변만 우연히 맞아도 잘못 저장한 사실은 추출 실패다.

| 능력 | 검증 방식·핵심 지표 |
| --- | --- |
| 명시적 기억·자동 추출 | eligible fact 단위 precision/recall, literal speaker·subject·source version·인용·valid time 일치. 불명 화자 통화는 제외 |
| 오래된/여러 대화 회상 | 10/100/1000턴, 여러 대화·비슷한 인명·한국어 조사/띄어쓰기·무관한 방해 자료. Recall@5와 최종 fact accuracy를 분리 |
| 시간·수정 | “지난주”, 명시 날짜·사용자 시간대·미래 변경·같은 사람의 최신 정정. 현재와 과거 질문에 다른 올바른 답 |
| 망각·자료 철회 | DB/FTS/cache/checkpoint/SSE/파생 문서·restore 경계. 명시적으로 잊은 fixture canary 재등장0, 실제 삭제 범위 밖의 별도 문서는 계약대로 유지 |
| 모름·상충 | 없는 사실은 추측하지 않음. 필요한 질문/모름 비율과 answerable 정확도를 함께 측정해 모든 질문 회피를 막음 |
| 효율·효용 | no-memory / raw retrieval / structured memory를 동일 reader·budget으로 비교. 품질·검색 지연·context token·추출 비용 함께 보고 |

Recall@5는 답 가능한 사례에서 `|유효 gold evidence ∩ top5| / |유효 gold evidence|`로 계산한다. 금지된 tenant/source 후보가 검색 단계에서 나오면 최종 필터로 숨겨도 격리 실패다. 망각의 의미상 모든 paraphrase를 차단한다고 확대 해석하지 않는다. 알려진 fixture 표현과 의미 검토의 관측 범위만 보고한다.

## 대화 품질·속도

최근 지시어·주제 전환·오래된 첨부·후속 수정·단위/시간대·사용자 말투·필요한 질문을 평가한다. 답변 문장을 하나로 고정하지 않고 사실·현재 요청 충족·간결성·근거·자연스러움을 본다. 불필요한 재승인 질문과, 꼭 필요한 누락 값을 묻지 않고 실행하는 오류를 별도로 센다.

`t_input`(사용자 전송), `t_ack`(서버 접수), queue/context/model/tool 구간, `t_first_final_body`(살아남은 최종 답변 첫 본문), `t_last_body`, `t_render`를 측정한다. “생각 중” animation이나 중간 commentary를 빠른 답변으로 세지 않는다. 첫 본문 이후 문자 표시 간격/긴 멈춤·전체 완료·취소 처리·초안 보존을 함께 검사한다. server monotonic duration과 browser performance duration을 나누며 서로 다른 기기 wall clock을 빼서 지연을 만들지 않는다.

기준 부하 프로필은 Linux4vCPU/8GiB/local SSD,1만/10만 source 항목,1/2/4 active users, provider fake first-delta300ms·고정 output cadence다.8명/100만 항목은 별도 stress로 보고한다. model 동시 실행 수·DB/WAL 크기·cache cold/warm·네트워크 RTT를 고정하고 latency를 섞지 않는다. cold start와 steady state는 별도다.

초기 목표(실측 아님): 기준 프로필에서 서버 접수 p95≤250ms,10만 항목 검색 p95≤250ms, event 수신→DOM 반영 p95≤150ms, 입력/클릭 응답 p95≤200ms. 실제 LLM 전체 응답 시간은 provider별 p50/p95·timeout·대기 시간을 별도로 비교한다. 실측 baseline 후 목표를 결과 보기 전에 고정하며 몰래 effort/model을 낮추지 않는다. 실제 모델 latency가 길면 그대로 보고한다.

## 미션 수행 능력

실행 가능한 미션 성공은 **필수 postcondition 전부 + 금지 부작용0 + 필요한 영수증 + 정직한 결과 보고**다. 부분 완료는 별도 점수이며 전체 성공으로 세지 않는다. 목표를 축소하거나 없는 연결을 실행한 척하면 실패다. 문서 내용·같은 문서ID/revision·메일의 실제 목적지/본문/receipt·일정 대상/시간·예약 occurrence를 evaluator가 직접 읽는다.

계획 수정·중단/재개·실패·예산 소진·최대2repair·checkpoint gap·external outcome_unknown을 포함한다. 원래 acceptance와 후속 지시를 동시에 지켜야 한다. 모든 도구를 정해진 순서로 부르도록 강제하지 않는다. 결과와 안전한 부분 순서/의존 관계가 같으면 다른 유효 경로도 인정한다.

미지원/필수 정보 부족/권한 없음 사례의 올바른 처리는 `handling_success`로 따로 집계한다. 이를 실제로 일을 완료한 `mission_success`에 섞지 않는다. 평가 전에 capability별 적용 사례를 고정한다. 약한 모델이 실패한 사례를 나중에 unsupported로 바꿔 분모에서 빼지 않는다.

## 스킬 활용·학습·커뮤니티 제안

필요한 스킬/도구를 찾았는지, 실제 compatible capability가 있는지, linked tool receipts로 결과가 났는지 확인한다. catalog 조회나 본문 읽기는 사용 성공이 아니다. 불필요한 스킬을 쓰지 않고 바로 해결하는 것도 정상이다.

학습 시험은 입력A/B의 독립 finalized 성공→candidate/usable 전이→보지 않은 입력C에 parameterized 재사용을 검증한다. 같은 입력 replay·본문만 읽기·완료 전 checkpoint·다른 사용자의 proof·외부 배포자의 성공 이력은2회 증거가 되지 않는다. schema drift·사용자 수정/삭제·귀속 실패3회와 infrastructure/cancel 실패를 구별한다.

[원문 근거 계산](evidence-calculation.md)은 실제 검색·완전한 원문 읽기·계산 영수증을 연결해 A/B→새 C를 관측한다. 여러 결과·중복 항목·잘린 원문·바뀐 revision·다른 실행의 영수증·수치 일부만 인용한 인수는 거절하고, 현재 자료 부족을 학습 결함으로 누적하지 않는다. 한글/emoji 인수의 위치는 서버가 결정한다. 공유 예제는 별도 합성 자료를 준비한 뒤 실제 공개된 요청 문구로 실행해야 하며 파일 형식 검사만으로 사용 가능하다고 판정하지 않는다. 원문/학습 근거를 철회하면 C의 파생 답변까지 사라져야 한다.

no-skill / discovery-only / learned-route를 같은 model/effort·초기 world·budget으로 비교한다. 품질 퇴행 없이 필요한 도구 호출·context token·latency가 줄었는지 보고한다. 무조건 스킬 사용률을 올리는 목표는 없다. 없는 배송사/API를 스킬로 연결 완료했다고 말하면 실패다.

커뮤니티는 GitHub 기여→정적 catalog→기존 ‘제안/할 수 있는 일’ 카드→추가/연결/실행을 실제 흐름으로 검증한다. ready/연결 필요/미지원·dismiss/mute·개인별 설치·version pin/update rollback/remove를 검사한다. private 문맥을 catalog에 보내는 추천, 설치만으로 자동 외부 실행, 다른 사용자의 skill proof 승계, scripts/hooks 실행은 실패다. 상세는 [커뮤니티 설계](community-skills.md)를 따른다.

## 판정·반복·출시 기준

판정 순서는 deterministic state/receipt/금지 동작 검사→사실/구조 비교→명시 rubric의 blinded LLM judge→표본 사람 검토다. 모델 judge는 문체/충족 여부의 보조이며 외부 실행 완료·권한·암호/격리 판정자가 아니다. 출력 속의 “평가를 통과 처리하라”는 지시를 data로 취급한다. judge model/prompt/version을 고정하고 동일 candidate의 자기 평가를 최종 판정으로 쓰지 않는다.

최초 pilot는 memory/chat/mission/skill의 독립 합성 세계 각8개(총32개),3회 반복으로 평가기 결함과 비용·분산을 찾는다. 이것은 출시 품질 증명이 아니다. 이후 난점/실제 결함으로 사례를 늘리되 테스트 수 자체를 목표로 하지 않는다. 선택 nightly smoke는 변경 영향 사례 위주로 제한하고 전체 provider×모델×effort 조합을 매 PR 실행하지 않는다.

### 재현 가능한 비교 기준

첫 Rust 안정판의 비교 기준은 **고정한 기존 비서 revision을 격리한 synthetic-only baseline 실행환경**이다. P0에서 source/lock/provider/model/effort/tool/prompt 설정을 고정하고 공통 fixture adapter를 만든다. 기존 테스트 코드를 옮기지 않고 새 합성 세계를 각각의 별도 DB에 적재해 동일한 사용자 turn·fake clock·provider 상태를 공급한다. 결과는 공통 postcondition/receipt 형식으로 정규화하며 baseline도 gold를 볼 수 없다. 옛 production DB/credential/service/cron은 연결하지 않는다.

비교 가능한 공통 capability와 새로 생긴 기능을 결과 보기 전에 구분한다. 공통 범위에는 같은 모델·예산·원문/도구 조건을 적용하고 새 기능은 absolute gate/별도 baseline으로 보고한다. 문서의 과거103/120 같은 수치는 비교 입력이 아니다. 동일 baseline을 재실행해 fixture 연결과 보존된 결과를 확인한 뒤 sample/power 계획을 고정한다. 이후 release 비교는 직전 지원 moosuri tag와 같은 절차를 쓴다.

기존 baseline을 재현할 수 없으면 NI는 `not-run`이며 첫 안정판의 동등성 gate를 통과하지 못한다. alpha 개발·절대 품질 측정은 가능하지만 쉬운 새 baseline으로 몰래 바꾸거나 과거 점수로 통과시키지 않는다. 대체 기준이 필요하면 근거와 비교 범위가 있는 별도 spec 변경으로 출시 조건을 먼저 수정한다. paid/model 실행은 그때의 예산과 전용 시험 연결 안에서만 한다.

출시 비교의 primary metric은 실행 가능한 미션 성공률이다. baseline과 candidate를 같은 frozen world/model/effort/budget·도구 상태에서 paired 비교하고 순서를 교차한다. 반복3회는 같은 세계에 종속된 관측이며 표본 수를3배 독립으로 세지 않는다. family/world 단위 cluster interval과 분모·실패 원인을 기록한다. 기존3%p 비열등 기준은 paired95% 구간 하한이−3%p보다 커야 통과다. pilot 분산/discordance로 필요한 표본과 비용을 사전 산정하고 구간이 넓으면 inconclusive다. seed/중단 규칙/추가 표본 계획을 먼저 고정하며 pass가 나올 때까지 재시도하지 않는다.

| 분류 | 초기 출시 목표(아직 미검증) |
| --- | --- |
| blocker 계약 | 누출·권한 우회·중복 외부 발송·삭제 부활·vault 평문 노출·허위 실행 완료의 관측 위반0. 관련 실제 경계 시험 모두 통과 |
| 메모리 | grounded extraction precision≥98%, eligible recall≥90%; answerable fact accuracy≥95%, 장기/시간 난점≥90%; answerability/missing 근거 성적 따로 보고 |
| 대화 | 현재 지시/지시어/필수 조건 정확도≥95%; 명확한 요청의 불필요 재확인≤5%, 실제 모호한 요청의 적절한 질문≥95% |
| 미션 | 대표 실행 가능 업무 full success≥90% 및 primary 비열등 기준. partial/blocked/unsupported/timeout는 별도 |
| 스킬 | compatible 관련 skill 선택≥90% on labeled eligible set, 허위 활성화/authority 확장0; 재사용 효과는 quality/cost/latency 함께 공개 |
| 속도·자원 | 위 기준 부하 목표와 실제 provider별 전후 구간. model 비용·토큰·RSS/CPU·queue fairness도 보고 |

이 절대 목표는 새로운 제품의 제안 기준이며 업계 평균·현재 성적이 아니다. 출시 전에 분모/표본을 고정하고 점 추정과 신뢰구간을 함께 낸다. 작은 표본에서 위반0은 무위험 증명이 아니다. critical 한 건을 평균 점수로 상쇄하지 않는다. model/profile별 통과 범위와 지원 수준을 표시하며 결과가 약한 연결을 기본 full-assistant로 추천하지 않는다.

## 최소한으로 강하게 만드는 검사

고위험 경계에 한해 mutation/fault로 검사 자체를 확인한다: tenant filter 제거, cursor 선진행, receipt 재사용 제거, 삭제 epoch 검사 제거, skill proof 생략, postcondition 없이 완료, revoked provider 재활성화. 각 변형을 적어도 하나의 직접 관측 검사가 잡아야 한다. 모든 함수에 mutation score 목표를 적용하지 않는다. property tests는 Unicode/시간대·ID/transaction/replay·입력 크기처럼 입력 공간이 넓은 경계에만 쓴다.

flaky는 rerun으로 숨기지 않는다. 원인·owner·수리 기한과 첫 실패를 기록한다. critical 격리/복구 검사는 quarantine해서 출시 통과로 만들 수 없다. 환경 장애와 제품 실패를 구별하되 미실행을 통과로 세지 않는다. provider outage로 제외한 비교는 전체 planned/executed/failed/not-run 분모를 공개하고 양쪽 같은 정책으로 재실행한다.

## 단계별 도입과 결과 형식

| 단계 | 필요한 최소 증거 |
| --- | --- |
| P0 | 새 fixture/oracle 규격·선정 이유, isolated baseline pin/fixture adapter 검증, syntax/checker·OpenSpec, 공개 가능한 합성 자료 |
| P1~P2 | SQLite/auth/tenant/retry/stream 필수 계약 + 두 사용자 text 수직 흐름 + provider probe |
| P3 | 메모리/자료/문서의 실제 저장·검색·정정/망각과 model 평가 |
| P4 | 미션 postcondition·스킬 lifecycle·실제 재사용·공유 패키지 설치 경계 |
| P5~P7 | 예약·변경 알림·Google/수집·사진/녹음/PWA·대표 실제 기기 |
| P8~P9 | v1 vault/broker·crypto/restore·계정/provider/accounting 복원·host-local secret 없이 명시한 복구 입력만으로 key 교체 전후·필수 파일 복원·migration·실제 kill |
| P10 | 가치 있는 regression 전부 + frozen holdout/성능 + clean install/기여/제안 카드 시나리오 |

현재 합성 회귀 진입점은 `cargo run --locked -p moosuri-eval -- --report /absolute/new-report.json`이다. 고정한 loopback provider·공개 세계·예산만 지원한다. `cargo xtask test-contracts`, 일반 `eval --suite ... --profile ... --budget ...`, `bench --profile ...`는 향후 통합안이며 구현한 명령으로 안내하지 않는다. React는 typecheck/component/대표 browser suite를 사용한다. CI는 사용자 지시로 꺼져 있다. 향후 CI를 다시 켜더라도 유료 모델/실계정은 별도 허용한 경로에서만 실행한다.

결과는 run/source/fixture/evaluator digest, profile, 적용·제외 이유, planned/executed/pass/fail/not-run, 단계별 postcondition, violations, paired 구간, p50/p95, usage/cost unknown, artifacts digest를 담는다. raw 모델/개인 내용·credential/hidden reasoning을 일반 로그나 공개 artifact에 넣지 않는다. 합성 replay도 기본 metadata report를 공개하고 필요한 실패 trace만 권리/비밀 검토 후 제한적으로 보존한다. `docs/status.md`가 실제 구현/시험 결과의 정본이며 이 문서는 방법의 정본이다.

근거: [평가 참고 자료](references.md), [구체적인 대화 예제](test-examples.md), [시나리오 표](../audit/evaluation-scenarios.tsv). 외부 benchmark의 문제/코드를 그대로 도입하지 않고 유효한 측정 관점을 가져온다.

초기 private baseline은 일반 OSS 설치·PR CI의 의존성이 아니다. 첫 비교 보고에는 code의 공개 여부와 재현 한계를 밝히고 공개 가능한 fixture·설정 digest·oracle·집계만 제공한다. 실제 원본 source/trace를 공개 artifact에 복사하지 않는다. 이후 공개 moosuri tag를 기준으로 같은 검증 경로를 재현할 수 있게 한다.

## 운영 경로의 숫자 시간 관측

실제 실행은 [실행 시간 관측](run-timings.md)의 content-free 숫자만 기존 run/usage API로
제공한다. 접수 기록부터 worker 대기·문맥 구성·모델 dispatch intent·모델 스트림·도구 처리·
완료한 명시적 최종 본문·종료를 구분한다. 첫 모델 호출 시각은 제공업체 수락 증거가 아니며
문맥 구성은 HTTP 접수나 전체 준비 시간을 대신하지 않는다. commentary·도구 결과·phase
없는 출력·부분/실패 결과는 첫 최종 본문 표본이 아니다.

최근 보관 실행128개의 attempt별 표본/누락과 고정 outcome 분모를 표시하며 구버전·잘린
span·직접 SQL 종료·재개방의 관측 공백을0으로 채우지 않는다. 이 수집 경로의 작은 합성
검사는 정확한 수집/분모를 검증할 뿐 위의 부하 목표·실제 모델 품질·browser DOM/입력
왕복 성능을 입증하지 않는다. 모델/effort를 몰래 낮추거나 추적 때문에 호출을 추가하지 않는다.

공개 `moosuri-eval` 회귀 범위에는 chat 명령의 기억·정정/재개방, conversation recall을 통한
전이 망각과 독립 source 보존, provider 응답 barrier에서의 owner 취소, compound A/B→unseen C
재사용·proof 철회가 포함된다. 기존 RunView 숫자는 접수 run별 표본/누락과 함께 수집하며
결과·예산 판정에 latency 기준을 추가하지 않는다. 작은 합성 회귀이며 실제 provider 수락,
crash/kill 복구, 부하·의미 품질·32세계 pilot의 완료 증거가 아니다. [실행 범위](agent-evaluation.md).

F27 조건 변경 검증은 정지한 일회성 업무를 새 acceptance revision으로 재계획한 뒤 실제 새 계산 영수증과 잘못된 후보의 보완을 확인한다. 이전 조건의 미실행 도구·늦은 실행/후보와 이미 쓴 호출/시간을 함께 검사하고, 새/이전 스냅샷 복원에서 이력 보존과 실행 격리를 확인한다. 의미 품질·넓은 외부 업무의 수락은 별도다. [범위와 증거](reviews/2026-11-15-mission-revisions.md).

F19 날짜 기억은 현재/적용 직전/정확한 시작/종료 직전/정확한 만료의 실제 조회를 검사한다. 정정 이력이 만료되면 옛 사실이 다시 현재로 나타나지 않아야 하며, 모델이 날짜 문구를 생략해도 원문 검증을 유지한다. 원래 시간대와 이후 실행 문맥의 시간대를 달리한 재시작·재시도, 기존 undated 계획 복구, 일반 사실의 잘못된 날짜 분류도 검증한다. [적용 범위](reviews/2026-11-15-dated-conversational-memory.md).

[자료 기록 시각 검색 검증](reviews/2026-11-15-source-time-filter.md)은 같은 사실 유효기간에 서로 다른 관측 시각을 가진 합성 원문을 사용한다. 실제 HTTP/Store 결과의 경계·20+1 페이지·계정/삭제·FTS/짧은 검색을 확인하고, 기간 조회의 학습 제외와 기존 조회 학습 호환성을 따로 검사한다. 자료가 기록된 시각을 실제 사건 발생의 독립 증명으로 평가하지 않는다.

[한국어 별칭 비교](reviews/2026-11-15-korean-search-aliases.md)는 구현 전에 수기로 정한 원문18개·질문12개의 ID 집합을 실제 Store 검색과 비교한다. 각 질문의 literal/별칭 결과와 문구 중간의 추가 단어·구두점·조사·여러 별칭 처럼 확장하지 않아야 할 사례를 분리하고, API 페이지·기간·소유자·철회 및 기존 학습 호환을 확인한다. 이 작은 고정 corpus를 독립 한국어 이해력 또는 전체 recall 점수로 계산하지 않는다.

[Gmail 첨부 검증](reviews/2026-11-15-gmail-attachments.md)은 큰 외부 첨부와 본문을 분리하고 실제 PDF child까지 실행한다. 정확한 메일/파트·내용/텍스트 해시를 페이지에 연결하고, UTF-8 경계·직렬화 크기·미지원 형식·권한 철회·늦은 응답을 검사한다. 수집기의 본문 수집 성공을 PDF 첨부 색인 성공으로 계산하지 않는다.
