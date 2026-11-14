# 개인 작업에서 배운 제한된 절차

개인 학습은 실제로 수행한 **정확 계산** 또는 **저장 자료 찾기**를 새 입력에
재사용한다. 공개 패키지 본문을 읽거나 설치하는 일은 학습 성공이 아니다.
현재는 일반 의미의 미션 성공·배송 조회·웹·메일·Calendar·코드 실행을 학습하지 않는다.
공유용 합성 패키지 export, 예약 연결과 자동 GC는 후속이다.

## 지원하는 정확한 모양

- `decimal_calculate` 한 번: 합·차·곱 중 실제 성공한 연산을 고정하고 새 피연산자를 받는다.
  기존 소수 정밀도·크기·overflow 검사를 그대로 쓴다. 숫자의 단위와 사용자의 의도는
  모델이 현재 요청에서 확인한다. 계산 route 자체가 자료의 숫자 추출을 증명하지 않는다.
- 저장 원문 `knowledge_search` 한 번 → 첫 hit의 현재 ID/revision으로
  `knowledge_read_source(start_byte=0,max_bytes=4096)` 한 번. 검색은 최대512 byte의 source literal 문구이며
  collection/subject/speaker/valid_at/cursor 필터를 쓰지 않는다. 새 문구로 다시 검색하고
  그 결과의 첫 원문을 읽는다. 첫 결과가 사용자가 원하는 대상을 뜻한다는 의미 판정은
  현재 요청에서 별도로 한다. 4096 byte보다 긴 원문은 재사용 시 잘림을 그대로 반환한다.

다른 실제 도구가 섞인 trace는 이 단위의 proof로 채택하지 않는다. `skill_read`는 무시하며
그 자체로 성공을 만들지 않는다. 두 원문 proof 모두 완전한 비어 있지 않은 excerpt여야 한다.
재사용은 foreground 대화에 한정하며 미션 실행기는 이를 허용하지 않는다.

## candidate에서 usable까지

동일한 현재 사용자/계정의 finalized run 안에 저장된 실제 입력·도구 결과의 연결과
독립적인 postcondition을 검사한다. 계산은 도구 결과를 정확 계산기로 다시 검산하고
최종 저장 답변에 그 정확한 숫자 token이 있는지 확인한다. 원문은 실제 첫 검색 hit와
읽은 ID/revision을 비교하고 최종 답변에 완전한 excerpt와 제목이 포함됐는지 확인한다.
따라서 사용하지 않은 성공 도구, `Completed`라는 문장 또는 완료 상태만으로 proof가
되지 않는다. 이 검사는 제한된 연산 결과의 답변 채택을 증명하며 의미상 업무 성공은 아니다.

첫 canonical 입력은 candidate, 다른 finalized run의 두 번째 canonical 입력은 usable이다.
숫자는 불필요한 0/소수 표기를 정규화하고 합·곱의 피연산자 순서를 무시한다. 검색은
공백을 정리하고 소문자화하며 다른 문구로 같은 원문을 읽어도 두 번째 독립 예제로 세지 않는다. 같은 입력의 replay, 완료 전 checkpoint, 다른 소유자와
배포자의 proof는 두 번째 성공이 아니다. proof는 최대 두 개의 input/receipt/원문 identity hash와
원래 run link만 보관하며 원문·인수·식별자를 별도 복사하지 않는다.

## 실행·철회·화면

새 run에서 usable pin과 짧은 설명을 기존 스킬 discovery에 고정한다.
`learned_skill_execute`는 그 pin과 새 `source.text` 또는 `decimal.operands`만 받는다.
학습 당시 A/B의 입력과 원문 ID는 route에 저장하지 않는다. 오래된 run은 새 pin을 얻지 않는다.
계산은 기존 예산 1개, 원문 찾기는 검색+읽기 최대 2개를 예약하고 실제 호출 수만 차감한다.
원문이 없는 경우 검색 1개만 차감한다. 잘못된 인수의 도구 결과는 최소 1개를 차감한다. 각 실제 호출의 입력·결과는 부모 slot에 연결된
private child receipt이며 동일 slot replay는 다시 실행하거나 예산을 차감하지 않는다.
기존 현재 사용자/권한·run fence·6 model/12 tool/120초와 근거 의존성을 유지한다.

기존 제안/할 수 있는 일 화면에서 검증 중·사용 가능·중단 상태를 보고 숨기기·비활성화·삭제한다.
삭제는 proof를 지우고 tombstone을 남겨 같은 절차가 자동 재생성되지 않게 한다.
비활성화/삭제는 실제 사용했거나 호출을 저장한 진행 run의 늦은 출력도 철회한다.
발견 문맥에만 pin이 있는 무관한 대화는 철회하지 않는다. 이전 답변이 실제로 후속 모델 문맥에 전달되면
그 후속 답변도 같은 철회 의존성을 이어받는다. 도구 없는 글 전용 후속 대화와 재시도에도
적용하며 새 스킬 pin이나 실행 권한은 부여하지 않는다. 원문 수정·삭제/망각은
기존 dependency purge 안에서 관련 proof hash를 즉시 지우고 파생 route의 답변/slot까지
철회한다. cascade는 저장된 run마다 최대 한 번 방문한다. 학습 예제가 담긴 대화를 삭제하면 그 대화의 proof만 지우고 남은 독립 예제가 하나이면
candidate로 되돌린다. 실제 사용하거나 호출을 저장한 파생 실행은 철회한다.
schema 변경·proof 무효화는 이전 route를 실행하지 않는다. 무효화된 절차는 이후 새 실제 성공을 모아 다시 학습한다. 두 개의 새 독립 proof가 필요하며
삭제 tombstone·사용자 비활성화·실패 격리는 이 경로로 해제하지 않는다. 일반 route 수정 UI는 후속이다.

현재 route에 귀속되는 잘못된 인수·산술 overflow 도구 결과가 3회면 quarantine한다.
provider/인프라 실패·취소·자료 없음·schema 무효화는 이 실패 수를 늘리지 않는다.
내부 검증은 추가 사용자 승인 단계가 아니다. private trace의 공개 전송·자동 게시가 없다.

검증 코드는 `agent_execution/tests/skills.rs`의 learned 시나리오에 있다.
실제 실행 결과와 실모델 품질/속도 비교 여부는 [현재 상태](status.md)에 별도로 기록한다.
