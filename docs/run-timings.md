# 서버 실행 시간 관측

내 사용량과 기존 관리자 계정별 사용량에서 최근 보관 중인 실행128개의 중앙값·95백분위,
표본 수와 누락 수를 확인한다. 계정별 원문·대화/도구 내용·URL·credential·모델/사용자/run
식별자는 timing DTO에 들어가지 않는다. 기존 계정별 사용량 화면의 계정 이름/메일 권한은
유지하며 새로운 개인 대화 조회 권한을 주지 않는다. 별도 telemetry나 모델 호출은 없다.

| 숫자 | 실제 관측 범위 |
| --- | --- |
| queue_ms | 같은 writer transaction의 신규 접수 기록부터 최초 worker claim까지 |
| context_ms | Store의 대화 문맥 조립. HTTP 파싱/인증·전체 실행 준비 시간이 아님 |
| first_dispatch_ms | 접수 기록부터 최초 durable model-call intent까지. socket/provider 수락 증거가 아님 |
| model_ms | 실제 Runtime 모델 스트림 처리 구간들의 합. transport·server backpressure 포함 |
| tool_ms | 실제 공통 도구 handler 구간들의 합. Store/cache·외부 대기·권한 확인/cleanup 포함 |
| first_final_body_ms | 완료한 attempt의 살아남은 명시적 final_answer 본문 첫 관측. phase 없는 본문·해설·refusal·도구 결과·부분/실패 결과는 제외 |
| terminal_ms | 실제 공통 종료 전이를 관측한 시각까지. 직접 SQL 종료 경로는 임의로 보충하지 않음 |

Runtime은 버퍼링 전 첫 비공백 body mark를 기존 guarded 출력 저장과 연결한다. 뒤늦게 phase가
확정되면 그 본문의 원래 mark를 사용한다. Store에서 직접 생성한 확인문은 최초 저장 관측을
쓴다. 이미 기록된 body ordinal의 반복 patch는 timing JSON을 다시 쓰지 않는다. 버퍼링된
body를 실제 SSE로 전달하거나 browser가 그렸다는 뜻이 아니며 DOM/브라우저 왕복은 측정하지 않는다.

시계는 프로세스 monotonic이다. system wall clock 차이를 빼서 latency를 만들지 않는다.
구버전 실행은 timing이 없으며, 재개방 당시 진행/대기 실행은 clock discontinuity를 표시하고
cross-open 전체/첫 최종 본문 시간을 채우지 않는다. 이미 관측한 짧은 model/tool 구간은
그 사실대로 남긴다. 시작만 있고 끝을 보지 못한 span의 합계는 null이며 model_calls/
measured_model_calls, tool_calls/measured_tool_calls가 각각 분모를 보여 준다. 실제0ms
관측과 값 없음은 서로 다르다.

`RunView.timings`는 현재 원래 attempt의 숫자다. `UsageSummary.timings`는 그 계정의 최근128개
보관 실행, 각 최대32개 원래 attempt에 대한 고정 outcome·sample/missing 집계다. 실패·취소·
결과 불명·일부 결과·거절·진행/대기는 빠지지 않는다. 과거에 관측하지 않은 attempt outcome은
unobserved이며 현재 성공으로 추정하지 않는다. percentile은 표본이 하나도 없으면 null이다.
인증된 기존 `/usage`와 관리자 `/admin/usage`를 쓰고 별도 추적/export endpoint는 없다.

새 숫자는 기존 private run JSON의 독립 extension에 저장한다. production의 model 쓰기 경로는
최초 chat/missions insert와 이 extension의 full-Value 갱신뿐이다. StoredRunChoice 읽기/직렬화가
기존 model을 덮어쓰는 경로는 없으며, 원래 model/effort/profile/다른 extension을 보존한다.
새 migration0048이나 dependency/framework는 필요하지 않다. 앞으로 model을 다시 쓰는 코드도
이 보존 계약을 지켜야 한다.

명시적 retry는 원래 attempt 식별자와 이전 숫자를 남기며 토큰/도구/시간 예산을 초기화하지 않는다.
실제 run/thread 행을 삭제하면 그 run JSON의 timing도 함께 삭제된다. 원문 철회나 출력 purge가
chat_runs 행을 남기면 숫자 관측과 accounting도 남을 수 있다. 행 삭제 후에도 남는 기존
model_call_receipts 토큰 집계와 이 보관-history 시간 창은 범위가 다르다. 시간 값 자체만으로
성공·실제 외부 실행·비용·모델 품질이나 전체 성능 목표를 통과했다고 판단하지 않는다.
