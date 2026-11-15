# 개인 작업에서 배운 제한된 절차

개인 학습은 실제로 수행한 **정확 계산**, **저장 자료 찾기**, **원문 근거 계산**, **일본우편 최근 상태 조회**를 새 입력에
재사용한다. 공개 패키지 본문을 읽거나 설치하는 일은 학습 성공이 아니다.
배송 조회는 [Japan Post 최근 요약](learned-parcel.md)의 고정 절차에 한정한다. 일반 의미의 미션 성공·다른 웹 업무·메일·Calendar·코드 실행은 학습하지 않는다.
계산/저장 자료 조회·일본우편 조회와 명시적 최대4단계 정확 계산 그래프의 공유용 합성 패키지 내보내기를 지원한다. 검증된 로컬 절차의 일회성·반복 예약 실행도 지원한다. 일반 외부 복합 절차와 자동 GC는 후속이다.

## 지원하는 정확한 모양

- `evidence_calculate`: 현재 요청의 유일한 검색 문구·숫자 항목 앞 문구·나머지 수치를
  인용해 한 원문에 묶은1–4단계 정확 계산을 실행한다. 실제 이전 검색·읽기 영수증과
  완전한 현재 원문이 있어야 하며 여러 검색 결과의 첫 항목을 임의로 쓰지 않는다.
  모델이 숫자나 한글 byte 위치를 추측하지 않고 서버가 원문 수치와 위치를 결정한다.
  두 다른 원문의 채택된 결과 뒤 새 원문을 다시 검색/읽어 재사용한다.
  [인수·근거·예산·공유 계약](evidence-calculation.md)을 따른다.

- `parcel_track` 한 번: 서로 다른 현재 번호의 최종 답변에 실제 조회 결과가 채택되면
  Japan Post 최근 요약 절차를 학습한다. 새 요청에 배송사와 유일한 새 번호가 있어야 하며,
  과거 번호/상태를 재사용하지 않고 실제 외부 조회를 수행한다. 전체 이력·다른 배송사·
  백그라운드 추적은 제외한다. [proof·재사용·공유 계약](learned-parcel.md)을 따른다.

- `decimal_calculate` 한 번: 합·차·곱 중 실제 성공한 연산을 고정하고 새 피연산자를 받는다.
  기존 소수 정밀도·크기·overflow 검사를 그대로 쓴다. 숫자의 단위와 사용자의 의도는
  모델이 현재 요청에서 확인한다. 계산 route 자체가 자료의 숫자 추출을 증명하지 않는다.
- `decimal_workflow` 한 번: 합·차·곱의2–4단계 DAG다. 입력2–32개와 전체 operand reference32개 이하이며,
  각 operand는 `{kind:"input",index:N}` 또는 `{kind:"result",index:N}`로 현재 입력/이전 결과를 명시한다.
  모든 입력·연산은 마지막 결과에 실제로 기여해야 하며 self/forward reference·임의 코드·graph 안의
  숫자 상수는 거부한다. 가격·수량·세율/할인 factor 등 모든 숫자는 현재 요청에서 새로 받아야 한다.
  외부 입력의 소수6자리·크기·overflow 검사는 유지하며 중간 계산으로 생성된 더 긴 정밀도는
  coefficient/scale로 정확하게 이어 계산한다. 단위/의미·누락값 추출의 성공을 증명하지 않는다.
- 저장 원문 `knowledge_search` 한 번 → 첫 hit의 현재 ID/revision으로
  `knowledge_read_source(start_byte=0,max_bytes=4096)` 한 번. 검색은 최대512 byte의 source literal 문구이며
  collection/subject/speaker/valid_at/cursor 필터를 쓰지 않는다. 새 문구로 다시 검색하고
  그 결과의 첫 원문을 읽는다. 첫 결과가 사용자가 원하는 대상을 뜻한다는 의미 판정은
  현재 요청에서 별도로 한다. 4096 byte보다 긴 원문은 재사용 시 잘림을 그대로 반환한다.

위에 정의한 조회·계산 흐름 밖의 실제 도구가 섞인 trace는 proof로 채택하지 않는다. `skill_read`는 무시하며
그 자체로 성공을 만들지 않는다. 두 원문 proof 모두 완전한 비어 있지 않은 excerpt여야 한다.
로컬 자료 조회·정확 계산·계산 그래프·원문 근거 계산은 대화와 명시적으로 맡긴 일회성/반복 업무에서 재사용한다. 배송 조회는 현재 대화에 한정한다.

## candidate에서 usable까지

동일한 현재 사용자/계정의 finalized run 안에 저장된 실제 입력·도구 결과의 연결과
독립적인 postcondition을 검사한다. 계산은 각 중간/최종 도구 결과를 정확 계산기로 다시 검산하고
최종 저장 답변에 그 정확한 숫자 token이 있는지 확인한다. 원문은 실제 첫 검색 hit와
읽은 ID/revision을 비교하고 최종 답변에 완전한 excerpt와 제목이 포함됐는지 확인한다.
따라서 사용하지 않은 성공 도구, `Completed`라는 문장 또는 완료 상태만으로 proof가
되지 않는다. 이 검사는 제한된 연산 결과의 답변 채택을 증명하며 의미상 업무 성공은 아니다.

첫 canonical 입력은 candidate, 다른 finalized run의 두 번째 canonical 입력은 usable이다.
숫자는 불필요한 0/소수 표기를 정규화하고 합·곱의 피연산자 순서를 무시한다. 그래프 proof도 입력을 넣은 식의 canonical hash를 비교하며, 숫자가 우연히 같다는 이유로 별도 계산들 사이의 인과 edge를 추측하지 않는다. 명시적 그래프 도구만 그래프 proof가 된다. 검색은
공백을 정리하고 소문자화하며 다른 문구로 같은 원문을 읽어도 두 번째 독립 예제로 세지 않는다. 같은 입력의 replay, 완료 전 checkpoint, 다른 소유자와
배포자의 proof는 두 번째 성공이 아니다. proof는 최대 두 개의 input/receipt/원문 identity hash와
원래 run link만 보관하며 원문·인수·식별자를 별도 복사하지 않는다.

## 실행·철회·화면

새 run에서 usable pin과 짧은 설명을 기존 스킬 discovery에 고정한다.
`learned_skill_execute`는 그 pin과 새 `source.text`, `decimal.operands`, `workflow.inputs`, `parcel.tracking_number` 또는 `evidence.parameters`의 현재 query/prefix/scalars 인용만 받는다. 복합 route에는 canonical typed graph만 저장하며 과거 operand나 문구는 저장하지 않는다. 발견 metadata에는 새 값의 slot을 맞출 구조만 포함한다. 계정별 route/tombstone은 최대64개다.
학습 당시 A/B의 입력과 원문 ID는 route에 저장하지 않는다. 오래된 run은 새 pin을 얻지 않는다.
단일 계산과 일본우편 조회는 기존 예산1개, 복합 그래프는 실제 연산마다1개, 원문 찾기는 검색+읽기 최대2개를 차감한다. learned 그래프는 보수적으로4개 남은 headroom을 확인하고 실제 연산 수만 차감한다. 원문 근거 계산은6개를 확보하고 실제 검색·읽기·1–4개 연산의 비용을 차감한다.
원문이 없는 경우 검색 1개만 차감한다. 잘못된 인수의 도구 결과는 최소 1개를 차감한다. 각 실제 호출의 입력·결과는 부모 slot에 연결된
private child receipt이며 동일 slot replay는 다시 실행하거나 예산을 차감하지 않는다.
기존 현재 사용자/권한·run fence·6 model/12 tool과 근거 의존성을 유지한다.
일반 chat의120초와 mission의 원래 최대하루 deadline을 구별하며, 재사용/재개로 늘리지 않는다.

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
삭제 tombstone·사용자 비활성화·실패 격리는 이 경로로 해제하지 않는다. 일반 route 수정 UI는 후속이다. 현재 native toolset generation3는 이전 generation의 qualification proof index를 기존 schema-drift 정책으로 무효화하므로 새 독립 두 예제가 있어야 다시 사용할 수 있다. 원래 대화·관측·실행 영수증을 삭제하지 않는다. 공개 instruction package 설치 권한이나 성공 이력을 이전하지 않는다.

현재 route에 귀속되는 잘못된 인수·산술 overflow 도구 결과가 3회면 quarantine한다.
provider/인프라 실패·취소·자료 없음·schema 무효화는 이 실패 수를 늘리지 않는다. 원문 근거 계산의 현재 인수/자료 없음·모호함도 늘리지 않으며 실제 산술 overflow만 귀속 실패로 센다.
내부 검증은 추가 사용자 승인 단계가 아니다. private trace의 공개 전송·자동 게시가 없다.

검증 코드는 `agent_execution/tests/skills.rs`의 learned 시나리오에 있다.
실제 실행 결과와 실모델 품질/속도 비교 여부는 [현재 상태](status.md)에 별도로 기록한다.

## 절차 공유하기

제안 화면의 개인 절차에서 **절차 공유하기**를 누르면 공유할 네 파일을 미리 보고 하나의
`.tar` 파일로 내려받는다. 파일을 풀어 공개 Git 저장소에 커밋하면 기존 GitHub 추가 화면으로
다른 사용자가 설치할 수 있다. 앱이 자동으로 게시하거나 다른 사람에게 전송하지 않는다.

현재는 저장 자료 찾아 읽기, 일본우편 최근 요약 조회, 정확한 합계·차이·곱셈과 위의 bounded typed graph·원문 근거 계산 구조만 새로 만든다. 원문 근거 예제는 별도로 준비할 새 합성 자료 조건을 제공하며 설치가 자료를 만들거나 예제 수치로 대체 계산하지 않는다. graph export는 입력 slot과 연산/reference만 읽고 저장소가 생성한 합성 숫자로 새 예제를 만든다.
사용자의 문구·대화·원문·숫자·계정·credential·실행/성공 이력을 내보내거나 모델에 보내지 않는다.
리포지터리가 제공하는 새 합성 예제와 Apache-2.0 라이선스만 포함한다. 복사본을 설치한 사람은
자기 계정의 현재 도구와 연결로 실행하며, 원래 사용자의 학습 증거나 자동 실행 권한을 얻지 않는다.
이 경로는 일반적인 임의 복합 스킬 export나 기존 private 코드 공개가 아니다.
[구현·검증](reviews/2026-11-14-private-skill-sharing.md).

예약 지침 의존성·공개 snapshot 정리는 [스킬 lifecycle](community-skills.md#예약-의존성과-참조-없는-snapshot-정리)에
연결했다. 업무 생성 시 현재 소유자의 사용 가능한 로컬 절차만 정확한 pin으로 고정한다.
실제 읽기나 실행한 절차가 사라지면 그 예약만 `needs_skill`로 멈춘다. 단순 제안 목록에
보였다는 이유로 다른 예약을 중단하지 않는다. 다시 켠 뒤 사용자가 예약을 재개해야 한다.
업무 수행은 새 학습 proof가 아니며 개인 proof와 삭제 tombstone을 GC하지 않는다.
[예약 재사용 검증](reviews/2026-11-15-mission-recipes.md).

## 채택한 복합 보고서

저장 자료 읽기→source-bound 정확 계산→요청한 비공개 보고서의 별도 qualification 경로와
카드 편집·새 revision·합성 공유를 추가했다. 일반 미션 완료 제외는 유지하고, 소유자가
채택한 두 독립 결과만 검사한다. [복합 절차의 범위와 검증](compound-learning.md)을 따른다.
D의 원래 로컬 초안 revision/digest 채택은 별도 draft allowlist로 지원한다. 각 새 초안의 승인·발송과 provider 저장 효과, browser credential은 학습 node에 포함하지 않는다.
