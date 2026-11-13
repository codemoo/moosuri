# 맡긴 일

사이드바의 **맡긴 일**에서 일회성 요청, 지켜야 할 조건과 확인할 내용을 직접 입력한다.
일반 대화나 자료 속 지시는 실행 권한이 되지 않는다. 기본값인 ‘창을 닫거나 로그아웃해도
계속’은 이 요청에 한한 명시적 실행 허용이다. 이를 끄면 요청 당시의 실제 로그인 세션이
유효한 동안만 실행한다. 관리자는 다른 사용자의 결과를 읽거나 제어할 수 없다.

현재 범위는 보관 자료·기억 검색/읽기, 허용한 지침 스킬 읽기, 정확한 소수 계산,
설치에서 설정한 공개 HTTP 읽기/검색이다. 메일·브라우저·볼트·임의 코드·외부 쓰기는
실행하지 않는다. 해당 도구를 요구하면 지원하지 않는 작업으로 남기고 무의미한 수정을 반복하지 않는다. 결과는 해당 사용자의 맡긴 일 기록 안에 저장한다.

일반적인 내용·의미 검토는 ‘일부 결과 · 검토 필요’로 남는다. 모델의 완료 선언은 증명이
아니다. API의 정확한 소수 결과, 원문 ID·revision·인용문, 출력 형식 조건은 현재 실행의
실제 완료 영수증과 대조한다. 출력 형식 조건은 내용의 의미를 증명하지 않는다.
자연어 조건을 적으면 양식은 `user_review`를 포함한다. API에서 이를 생략해도 서버가
추가 조건의 검토를 요구하며 원래 요청은 바꾸지 않는다. 기계 검사가 모두 통과하면
추가 모델 수정을 반복하지 않고 결과를 검토 필요로 남긴다. 조건이 없는 계산 확인은
정확한 값의 영수증·출력이 일치할 때 선택한 확인 기준을 통과한 것으로 표시한다.

계획은 최대 다섯 단계다. 계획과 실행·검증·최대 두 번의 수정은 모델 여섯 번, 도구 열두 번의
같은 실행 한도를 사용한다. 요청 당시부터 하루가 지나면 만료된다. 정지·재시작·추가 지시는
시간이나 한도를 늘리지 않는다. 한 사용자당 진행/정지 중인 일은 하나, 설치 전체는 최대
열여섯 개를 기다릴 수 있고 동시에 실행하는 맡긴 일은 하나다. 일반 대화에 실행 자리를 남긴다.

잠시 멈춘 뒤 안정적으로 완료된 읽기/계산 결과는 이어서 사용할 수 있다. 모델·공개 HTTP 요청이
이미 시작된 순간 멈추거나 프로세스를 잃으면 그 요청을 자동 재실행하지 않는다. 응답이 이미 완료된 계획·결과 제안은 모델 영수증과 같은 트랜잭션에 명시적 자료로 보관하며, 재시작 때 그 자료를 확인하므로 같은 제안을 다시 요청하지 않는다. 결과 불명
안내를 확인하고 새 일로 맡긴다. 정지 상태에서 추가 지시를 할 수 있지만 처음 요청·조건·확인
기준은 변경하지 않는다. 취소는 이후 실행을 막으며 이미 발생한 사용량 영수증은 남는다.
자료·스킬·모델 연결 철회는 관련 결과와 실행 허용을 무효화한다. 계정 정지·인증 세대 변경 후
자동 재허용하지 않는다. 복원한 미완료 작업은 격리하고 새 요청을 받아야 한다.

API는 `POST/GET /api/v1/missions`, `POST /api/v1/missions/{id}/control`,
`DELETE /api/v1/missions/{id}`다. 모든 요청은 실제 세션과 `X-Moosuri-Account`를 검사한다.
생성 UUID를 재시도하면 같은 요청을 찾고 다른 내용이면 충돌한다. 제어는 UUID와 revision CAS를
사용한다. 제어 재시도는 현재의 철회 상태를 반영하여 과거 비공개 결과를 다시 노출하지 않는다.
삭제는 기존 파괴적 작업 저널에 영구 기록하며 재시도·복원으로 삭제한 결과를 되살리지 않는다.

이 범위에는 반복 예약된 복합 작업, 변경점 비교, 대화에서 작업 자동 추출, 실행 범위 확장,
메일/브라우저 읽기, 의미 판단 자동 완료가 포함되지 않는다. F27 전체 완료를 의미하지 않는다.

## Acknowledged recovery and control retries

A stable acknowledged final proposal can finish local receipt verification after restart even at exactly6model calls or12tool calls. This exception requires the latest completed non-planning proposal and no unresolved tool/unknown dispatch. It does not authorize another dispatch or reset any original budget, deadline, account/provider/epoch, withdrawal or monotonic-clock fence. Ordinary chat retry admission is unchanged.

An uncertain owner control retains its UUID/payload only for an explicit retry of the same mission and exact action. Polling/current-state reads retire stale control revisions; a later Cancel or another card/instruction gets its own current revision and fresh UUID. A racing CAS rejection reconciles state and retries only that newly requested intent once. A failed current-state read stops the control instead of replaying an older action. No additional approval UI is introduced.

Stable mission controls and restarts cannot strand execution at the shared chat attempt32 boundary. Within the same writer transaction, mission continuations retire the old run lease and use a fresh nonce on claim; no old run/step capability can change the resumed result. Step accounting receipts remain available for late usage, and original model/tool/time limits remain unchanged. Ordinary chat retries retain their existing limit. Unknown dispatches never enter this path.
