# 맡긴 일의 첫 실행에서 불필요한 모델 왕복 제거

기준 `960f9a5`의 새 맡긴 일은 도구 없이 단계 제목만 받는 모델 호출을 먼저 썼다.
첫 호출부터 기존 실행 도구나 원래 업무의 누락값 질문을 사용할 수 있게 했다. 표시용 계획은
현재 승인한 작업의 문구와 실제 근거·완료 조건 확인 단계다. 긴 문구는 UTF-8 경계에서 줄인다.
현재 요청·조건·확인 기준은 모델 문맥에 그대로 있으며 표시용 제목은 실행 권한이나 완료 증거가 아니다.
대화의 현재 사용자 의도를 분리해서 해석하는 기존 호출은 유지한다. 모델·reasoning effort도 바꾸지 않는다.

## 저장과 재시작

첫 실제 모델 응답을 기존 StepLease로 확정한 뒤, 도구 실행 전에 같은 완료 step으로 계획과
보고서/action 계약을 허용한다. 새 RunLease 쓰기 권한이나 별도 예산은 추가하지 않았다.
모델 응답 확정 뒤 계획 저장 전 또는 저장 뒤 도구 진입 전에 중단될 수 있다. 재시작은
확정된 비계획 응답을 먼저 확인하고 같은 acceptance revision으로 허용한 뒤 미완료 슬롯만
실행한다. 계획 저장은 비계획 응답의 본문을 보존하고, 기존 consumer가 처리한 뒤 지운다.
과거 버전에서 이미 받은 단계 제목 응답은 기존 경로로 복구한다.

원래 여섯 모델 호출·열두 도구 호출·만료·두 번의 보완 한도, 인증 세대, provider·연결 철회,
정지·취소·결과 불명과 현재 acceptance의 step floor는 그대로다. 실제 메일 발송 승인은 추가하거나
생략하지 않는다. 실제 서비스, 운영 자료, 외부 메일 발송과 CI는 실행하지 않았다.

## 확인한 사용자 결과

실제 Rust Runtime, loopback 모델 프로토콜과 합성 SQLite를 사용했다. 기존 검사의 결과·실제 영수증을
유지하고 불필요한 호출 하나만 줄였는지 계수했다. 반복/재시작도 모델 사용량 DB와 함께 확인했다.

| 결과 | 이전 호출 | 수정 후 호출 |
| --- | --- | --- |
| 실제 저장 자료 조회·정확한 합계·재개방 후 결과 | 4 | 3 |
| 대화의 명시 요청 해석·두 원문·합계·비공개 보고서1개 | 5 | 4 |
| 예정 시각 이후 반복 계산 회차·알림 | 3 | 2 |
| 연결 Gmail/Calendar 조회·초안/Gmail draft·Calendar 작업, 발송0 | 6 | 5 |
| 누락 수신인 답변 후 같은 업무의 완료, 발송0 | 6 | 5 |

보고서 검사는 실제 원문 참조2개와 계산 영수증, 문서1개 저장, 재개방 및 원문 철회 시 파생
문서 철회를 유지했다. 새 복구 검사는 실제 loopback 응답을 먼저 확정하고 계획 저장 전/후에
Store를 다시 연 뒤 Runtime로 이어 간다. 두 경계 모두 모델2회·실제 자료/계산 슬롯2개·문서1개다.
최종 제안 보존 검사는 앞서 완료한 실제 계산 영수증과 확정된 최종 응답을 사용하며, 모델 주소가
응답 불가능한 상태에서도 재개방 후 검증을 마치고 기존 호출2회를 유지했다.

조건 변경 검사는 옛 원문과 현재 조건/영수증을 구분하며 틀린 결과의 보완을 원래 예산 안에서
끝냈다. 기존에 확정한 과거 단계 제목의 복구도 통과했다. 별도 연결 업무 검사에서는 늦은
연결 철회, 수신인/문맥 대기와 동일 업무 재개, 최종 승인 대기, 실제 발송0을 확인했다.
개인 모델의 직접/반복 업무, 연결 읽기 철회, 취소·철회·불명 dispatch 미재실행도 통과했다.

실행한 명령:

- `cargo test --locked -p moosuri-agent tests::missions:: -- --nocapture`: 최종 응답 복구 검사를 추가하기 전7개 통과.
- `cargo test --locked -p moosuri-agent tests::connected_missions:: -- --nocapture`: 5개 통과.
- `cargo test --locked -p moosuri-agent mission -- --nocapture`: 27개 통과·3개 실패. 이 중 새 최종 응답 fixture의 근거 누락은 아래처럼 바로잡았다. 나머지 두 실패는 아래에 보존했다.
- `cargo test --locked -p moosuri-agent mission_final_candidate_survives_local_plan_save_and_reopen_without_inference -- --nocapture`: 실제 계산 근거를 넣은 최종 fixture1개 통과. 근거가 없던 이전 실패를 제품 성공으로 바꾸어 세지 않는다.
- `cargo fmt --all`, `git diff --check`와 해당 OpenSpec strict validation 통과.

넓은 `mission` 필터에서 실패한 기존 두 검사는 이번 수정의 mission 실행 경로에 들어가기 전
현재 대화 의도 분류 단계에서 끝난다. 정본 통합 과정에서 깨끗한 기준 `960f9a5`의 각 단독 검사를 실행해
같은 실패를 재현했다. current-request 분류를 보완한 뒤 기존 Runtime assertion을
바꾸지 않고 최종 Linux에서 두 검사 모두 통과했다. 통과로 숨기거나 전체 통과라고 보고하지 않는다.

- `tests::mission_chat::delayed_saved_read_schedules_future_work_without_early_reads_or_literal_reminder_models`: 기대 `unsupported`, 실제 `clarify`.
- `tests::planner_composition::artifact_no_command_then_mission_skip_reads_actual_source_and_answers_without_mutation`: fixture가 예상하지 않은 mission classifier 호출을 받아 `connection_failed`.

정본 통합 후 Linux에서 `credential-broker-chat` feature를 켠 실제 서명 mail 재개 검사를
통과했다. 첫 실행·근거 계산·프로토콜 통합과 저장/권한 경계의 독립 검토도 수행했다.
최종 실모델 비교와 통합 결과는 [후속 기록](2026-11-15-source-repair-latency.md)을 따른다.

## 관측의 한계

이 결과는 정상 합성 경로에서 모델 dispatch 하나를 줄였다는 증거다. 실제 모델의 새로운
업무 품질이나 운영 응답 지연 개선율을 증명하지 않는다. 기존4차 임시 bridge는 매 호출의
실행기 생성·설정·이력 재입력을 포함하므로 그 초 단위 값을 제품 지연으로 바꾸어 쓰지 않는다.
운영에서는 기존 `RunView.timings`/사용량 화면의 model/tool/terminal 관측으로 비교할 수 있으며,
이 수정은 새 telemetry나 성능 framework를 추가하지 않는다.
