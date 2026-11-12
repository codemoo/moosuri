# 핵심 시나리오의 합성 대화 예제

아래는 새 테스트 작성용 명세다. 실제 테스트/모델 실행 결과가 아니며 정답의 문구는 여러 표현을 허용한다. evaluator는 답변과 함께 DB·artifact·tool receipt·금지 부작용을 확인한다. 사용자의 실제 대화를 복사하지 않았다.

## MEM04 — 현재 사실과 과거 사실

고정 시간은2027-03-10, 사용자 시간대는Asia/Seoul이다.

1. 과거 발화: “3월1일부터 월요일 오전에는 회의를 잡지 말아줘.”
2. 새 발화: “3월15일부터는 월요일 오전도 괜찮아. 14일까지는 그대로야.”
3. 질문: “3월12일 기준 내 월요일 오전 선호는?” → 회의를 피한다.
4. 질문: “3월16일 기준으로는?” → 잡아도 된다.
5. 질문: “2월에는 어땠지?” → 주어진 자료만으로 알 수 없다.

oracle: 두 fact의 유효 구간과 원문 version을 분리하고 미래 변경을 현재에 소급 적용하지 않는다. lookup은 맞았는데 답변이 반대면 실패다. 변형은 자정 경계·시간대 변경·서로 다른 동명이인·오래된 발화의 늦은 수집이다.

## MEM05 — 정정·망각이 재시작 뒤에도 유지

1. “선호 배송지를 ‘가상구 새길7’로 기억해.”
2. “주소를 ‘가상구 새길9’로 정정해.”
3. “지금 배송지는?” →9, 이전7을 현재 값으로 사용하지 않는다.
4. “배송지 기억을 잊어줘.”
5. 서버 재시작·예전 검색 page 재전송·잊기 전 checkpoint 재개 후 “배송지 알려줘.” → 잊은 값을 제시하지 않는다.

oracle: 새 revision·망각 tombstone/최신 journal과 DB/FTS/SSE/checkpoint/파생 자료를 검사한다. memory forget와 source delete 범위는 계약대로 구분한다. 백업 복원에 최신 journal이 없으면 서비스를 열지 않는다. 이미 기기에 전달됐던 bytes를 원격 삭제했다는 주장은 하지 않는다.

## CHAT04 — 실패·재시도에도 새 초안은 그대로

1. 사용자가 이미지와 “이 영수증에서 날짜와 합계 알려줘.”를 전송한다.
2. 서버 접수 뒤 ACK를 잃고 UI가 전송 불명을 표시한다.
3. 사용자는 입력창에 다음 질문을 쓰다가 원래 메시지의 재시도를 누른다.
4. 이전 attempt의 늦은 chunk와 현재 attempt의 정상 결과가 차례로 온다.

oracle: 원래 message/run/첨부 identity가 유지되고 새로운 중복 요청/중복 처리 없음; 새 초안·새 첨부는 보존; 이전 attempt 결과는 표시되지 않음; 이미지 미리보기와 최종 답변은 원래 메시지에 남음. fixed fake provider로 수명 계약을 검증한 뒤 실제 모델의 영수증 사실 추출을 별도 채점한다.

## TASK01/TASK02 — 조건을 지키고 실제 결과물 갱신

합성 메일3개에 후보 A/B/C의 가격·운영 시간·취소 조건을 넣는다. 모두읽기 연결만 허용한다.

1. “토요일 오전에 갈 수 있고 합계5만원 이내인 두 후보를 비교해서 문서로 저장해. 예약은 하지마.”
2. 작업 중 “예산은4만원으로 바꾸고 취소 조건도 포함해.”
3. 문서 생성 뒤 “둘 중 더 저렴한 걸 첫 줄에 적어.”

oracle: 최신 acceptance4만원·토요일 오전·두 후보·취소 조건·근거가 만족되고 같은 문서ID가 revision만 증가한다. 외부 예약/메일0. 하나만 해당하면 두 개를 지어내지 말고 해당 부족을 설명한다. verifier가 빠진 조건을 발견하면 같은 예산에서 최대2회 repair; 실패하면 부분/미완료로 보고한다. “완료” 문장만으로 pass하지 않는다.

## TASK05 — 발송 결과를 모를 때 다시 보내지 않기

명시된 합성 수신인 `receiver@example.invalid`과 고정 본문을 fake Gmail provider에 설정한다. 실제 이메일은 보내지 않는다.

1. “이 수신인에게 ‘내일 오전10시에 확인하겠습니다’라고 보내줘.”
2. provider는1회 발송 후 응답을 잃는다.
3. 사용자가 “다시 시도”를 누른 뒤 server를 재시작한다.
4. 같은 effect의 receipt 조회가 뒤늦게 성공한다.

oracle: send count=1, 같은 effect/Message-ID·checkpoint 연계, 불명 구간에는 완료 주장 없음, 최종 조회로만 확정. send-only scope 때문에 조회할 수 없으면 unknown을 유지한다. 새 ID를 만들어 재전송하거나 취소 후 늦은 영수증을 버리면 실패다.

## SKILL02/SKILL03 — 배운 절차를 새 입력에 사용

가짜 배송 조회 tool과 두 운송장 A/B, 세 번째 C를 제공한다.

1. A 성공1회 → candidate. A를 replay해도 별도 성공으로 세지 않는다.
2. B의 실제 linked tool results와 finalized 성공 → usable.
3. C로 “어제 이후 상태가 바뀌었는지 알려줘.” → C와 현재 근거로 실행한다.
4. tool schema version을 바꾸고 다시 요청 → 오래된 route를 조용히 실행하지 않는다.

oracle: A/B의 운송장·사용자·원문을 C에 고정하지 않음; source proof 없이 본문 읽기만으로 사용 성공 기록0; invalidated route는 새 증거/현재 도구로 해결하거나 제한을 설명한다. 받은 커뮤니티 패키지의 작성자 성공2회는 수신 사용자의 증거가 아니다. 무스킬 대비 결과 품질/호출량/token/시간을 함께 비교한다.

## AUTO03 — 변경 알림에서 불명을 숨기지 않기

“매일 오전9시에 이 공개 공지를 확인하고 바뀔 때만 알려줘.”

fake clock으로 네 회차를 진행한다: 최초 정상 결과→같은 내용/다른 요청시각→본문 변경→provider timeout. 기대 알림은 첫 회/변경/확인 불가이며 엄격히 동일한 둘째 회만 생략한다. 진행 중 하나+최신 대기 하나의 coalescing을 검사한다. 알림 재전송은 새 미션을 실행하지 않는다. topic mute는 버전 변경 후에도 유지한다.

## SEC04 — 오래된 백업이 권한·예산을 되살리지 않기

snapshot 이후 비밀번호 변경·admin 강등·provider revoke·유료 호출 reservation을 순서대로 기록하고 snapshot을 복원한다. oracle은 옛 hash로 새 로그인0·과거 role/provider 자동 활성화0·현재 disable/delete journal 유지·미완료 effect 재발송0다. post-snapshot usage를 알 수 없으면 accounting_unknown과 추가 지출 차단을 확인한다. 정상 restart에는 credential 재등록을 요구하지 않는다.

## COMM01~COMM06 — Git 기여가 제안 카드로 이어지기

1. contributor가 합성 예제와 compatible tools를 적은 skill PR을 올리고 공개 catalog에 반영한다(시험에서는 local Git fixture).
2. 연결 상태가 다른 사용자 A/B에게 같은 public catalog를 내려받는다. A의 카드에는 “추가하고 시작”, B에는 “메일 연결 필요”가 보인다.
3. A가 추가 후 명확한 읽기 작업을 맡긴다. 매 step 승인 없이 기존 tool 권한으로 실행한다. 설치만으로 메일 발송은 일어나지 않는다.
4. B가 제안을 숨긴 뒤 catalog version을 올려도 같은 주제가 재노출되지 않는다.
5. update가 중간에 실패하면 A의 기존 pinned version은 그대로 동작한다. A의 제거는 B에게 영향을 주지 않는다.

oracle: private 대화/검색어/기억은 registry 요청에 없고, title/reference의 HTML·script/hook은 실행되지 않는다. shell/알 수 없는 connector가 필요한 패키지는 “바로 가능”이 아니다. 개인 학습 export는 allowlisted route+합성 예제로 재구성하며 fixture에 심은 개인값/receipt payload가0개다. 실제 커뮤니티 게시/메일 발송은 이 테스트로 수행하지 않는다.

## CRED01~CRED05 — 연결한 계정으로 이어서 처리

1. 사용자 A가 “개인 메일로 이 합성 수신인에게 답장해줘”라고 요청한다. 개인/업무 계정 후보가 실제로 모호하면 한 번 고른다. 항목을 모델에게 읽어주지 않고 broker가 binding을 만든다.
2. 다음에 “같은 개인 메일로 이 답장을 보내줘”라고 명확히 지시하면 유효 binding과 현재 수신인/본문 범위를 확인해 추가 승인 없이 진행한다. 이전 사용에 관한 모델 기억만 있고 binding이 없으면 연결 완료로 간주하지 않는다.
3. trusted device가 잠기면 `waiting_for_credential`로 같은 요청을 보존하고 실행 슬롯을 반환한다. 다른 사용자의 대화는 계속된다. unlock 뒤에는 원래 요청이 아직 유효한 경우 자동으로 이어간다.
4. fake provider는 발송하고 ACK를 잃는다. broker A가 재시작하면 기존 device journal·인증된 영수증으로 같은 effect를 조회한다. 그 사이 broker B/다른 tab이 준비돼도 adopted owner/generation을 바꾸어 재발송하지 않는다. A의 늦은 영수증은 원래 effect로 보존한다.
5. credential 회전/철회와 사용자 B의 잘못된 handle 요청을 넣는다. stale grant/다른 사용자 실행0, upstream 철회 불명은 불명으로 표시한다.

oracle: strong-device mode에서 password/API/OAuth/TOTP canary가 server DB/model/log/영수증/스킬 export에0개; operation/resource/recipient·origin을 벗어난 요청0; 지원 TOTP adapter만 code 생성/사용, 코드가 채팅에 나타나지 않음; fake mailbox send count≤1 또는 명시적 unknown. 명시적으로 선택한 server-token 모드는 서버 token 보관을 허용하는 다른 경계로 따로 채점한다. 실제 볼트나 외부 계정으로 이 계획을 실행하지 않는다.

## AUTO01/AUTO02 — DST와 중단·수정의 정확한 결과

- `America/New_York` 매일01:30의2027-11-07 fold는05:30Z에1회,06:30Z에는0회다. 매일02:30의2027-03-14 gap은 그 회차를 skipped로 기록하며 다음 날06:30Z가 다음 실행이다.
- UTC09:00 anchor의1시간 간격 예약은 실행이09:20에 끝나도 다음은10:00이다.09/10/11시를 서버 중단으로 놓치고11:30에 복귀하면11시 회차1개만 delayed 실행한다.
- pause 기간은 resume 때 재생하지 않는다. claim 뒤 규칙을 수정하면 old revision의 dispatch commitment는 실패하고 다음 미래 회차부터 새 규칙을 쓴다. 이미 committed된 외부 작업의 receipt는 보존한다.

## OPS03 — DB 밖 첨부까지 빈 환경에 복원

합성 사진/PDF/문서 파일을 저장하고 snapshot/manifest/pin을 확정한다. 백업 복사 도중 사진을 삭제하고 GC·process kill을 주입한다. 완성된 archive만 빈 data_dir에 복원했을 때 보존된 PDF/문서는 원래 hash로 읽히고 thumbnail은 재생성되며, 삭제 사진은 최신 journal 때문에 노출되지 않아야 한다. manifest의 PDF bytes를 빼거나 바꾸면 DB integrity가 정상이어도 recovery_required다.

## CRED02/CRED03 — 정상 연결 채널도 권한의 정본은 아님

paired server가 새 grant를 위조하면 기기의 local grant를 확대하지 못한다. 기존 범위 안에서 서버가 task를 구성하는 위임의 한계도 별도로 기록하며 사용자 의도 위조를 pairing만으로 검출했다고 주장하지 않는다.

정상 서버 경합 시험은 prechecked 요청을 붙잡고 broker 연결을 끊은 뒤 계정을 정지한다. 나중에 요청을 풀어도 새 commitment는 실패해야 한다. 반대로 commitment가 정지보다 먼저라면 in-flight 결과가 나올 수 있고 같은 effect로 정산한다. 기기 철회 ACK가 없는데 완료로 표시하면 실패다.

## OPS03/VAULT05 — 원래 서버를 잃어도 백업을 열 수 있어야 함

합성 assistant/vault backup A를 만들고 outer key를 교체해 B를 만든다. 별도 시험 보관소에는 두 epoch의 backup identity, A/B archive, 최신 독립 journal/head와 필요한 검증 자료만 둔다. 기존 호스트의 data_dir/config/key·세션·provider credential을 모두 없앤 새 환경에서 그 자료만으로 A/B를 각각 복원한다. vault outer 해독 후에도 항목은 ciphertext이며 사용자 master/recovery 없이는 평문이 나오지 않아야 한다.

missing identity는 `backup_key_missing`, 잘못된 key/변조는 `backup_decryption_failed`다. 옛 journal segment가 old key를 요구하면 최신 key 하나만으로 통과시키지 않는다. 최신 journal이 없으면 key가 맞아도 recovery_required를 유지한다. 이 예제는 명세이며 실제 개인 키 생성·복원 시험 결과가 아니다.
