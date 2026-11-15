# 채택한 자료·계산·보고서 절차

지원하는 첫 복합 절차는 **새 저장 원문을 유일하게 찾기 → 정확 계산 → 명시적으로 요청한 비공개 보고서**다. A의 보고서 결과에서 **검토한 수치 결과를 채택해 절차 학습**을 누르면 현재 소유자의 실제 원문·계산 영수증과 정확한 문서 버전을 검증한다. 같은 typed 구조의 서로 다른 원문/현재 입력으로 만든 독립 결과 두 개가 있어야 기존 **제안 · 할 수 있는 일**에서 사용 가능해진다. 일반 미션 성공을 학습하는 기존 전체 제외는 유지한다.

수치·원문 title/prefix/관측 값의 문서 채택과 실제 저장을 확인한다. 문서 body는 최종 mission candidate와 같아야 하며 현재 active revision이어야 한다. 서버가 실제 search/read 결과에서 숫자를 재추출하고 모든 arithmetic child 영수증을 검산한다. 도구 성공 또는 모델의 성공 선언, 보고서 저장 버튼만으로 proof를 만들지 않는다. 의미상 비교·추천·누락 판단은 계속 `review_required`이고 미션의 `partial`/검토 상태도 바꾸지 않는다. 순수 요약처럼 독립적인 사실/수치 predicate가 없는 모양은 승격하지 않는다.

## 새 입력으로 이용하기

사용 가능한 절차는 새로 맡긴 일의 기존 discovery/pin에 들어간다. 사용자 요청에는 새 검색 문구, 숫자 앞의 정확한 문구, 모든 scalar, 보고서 저장 요청을 명시한다. 모델은 기존 `learned_skill_execute`의 `kind:evidence`와 새 parameters만 전달한다. 원문 조회와 숫자 추출은 서버가 다시 수행한다. 보고서 생성은 A의 현재 미션 요청과 별도 artifact mutation 영수증에 따르며, 절차가 저장 권한을 주지 않는다. foreground chat에 보고서 절차를 직접 호출하거나 보고서를 요청하지 않은 미션에서 호출하면 사용할 수 없다.

구조는 schema/revision과 `EvidencePlan`만 보관한다. 고정 search/read/계산/report 연결은 순환이 없고 실제 산술 1–4개를 포함해 최대7개 node, 도구 종류4개다. 전체 상한8node/9toolkind 안에 있다. graph에는 상수·코드·계정/resource/binding·원문·credential·과거 입력을 저장하지 않는다. 기존 source-bound DAG 검사, 소수 정밀도, 현재 메시지의 유일하고 겹치지 않는 인용, complete source 검사, 실제 child receipt를 그대로 사용한다. search/read/각 산술 호출은 기존12tool 예산을 차감하며 새로운 model/tool 예산을 만들지 않는다. 보고서는 A의 기존 단일 Store mutation이다. 실패한 현재 자료/인수는 과거 값이나 합성 예제로 대체하지 않는다.

기존 recipe의 실행 결과와 예약 회차는 새로운 proof가 아니다. imported instruction을 읽은 run도 proof가 아니다. 같은 run/정규화 입력/같은 원문을 다시 채택해도 두 번째 예제로 세지 않는다. content-free observation marker는 증거가 지워진 과거 결과를 다시 수확하지 못하게 한다. proof에는 해시와 원래 run link만 저장한다. 채택 테이블에는 기존 run/artifact identity·revision과 처음 채택 때 선택한 구조 route를 연결한다. 이 route는 고정되므로 다른 절차를 같은 계산 모양으로 편집해도 이미 채택한 proof나 그 pin을 사용한 예약은 새 revision으로 이동하지 않는다. 새 채택만 새 구조 revision을 선택한다.

## 변경·철회·예약

카드의 **계산 구조 편집**은 typed plan을 편집하고 새 recipe revision을 만든다. 기존 proof를 복사하지 않는다. 이전 pin은 tombstone으로 남고 실제 사용 결과를 철회하며 관련 예약을 `needs_skill`로 멈춘다. 이후 새로 실행하고 채택한 두 결과가 새 버전을 검증한다. 과거 A/B 결과를 다시 누르는 것으로 새 revision을 채울 수 없다. 기존64route/tombstone 한도를 적용한다.

원문/학습 대화 삭제, qualification 철회, 문서 편집·삭제·revision 변경은 proof와 실제 사용 결과를 무효화한다. 단순히 discovery에 pin이 있었던 무관한 대화/예약은 중단하지 않는다. 기존 disable/remove·실패 귀속/산술 overflow 격리·삭제 후 자동 재생성 금지를 유지한다. disable을 다시 enable하더라도 예약은 자동 재개하지 않는다. 사용자가 명시적으로 재개해야 한다. 편집해 사라진 옛 pin에 의존하는 예약은 새 업무로 만들어야 한다.

## 공유와 다음 경계

기존 **절차 공유하기**는 허용된 구조와 새 repository 합성 예제로 네 파일 패키지를 재구성한다. 원래 trace·소유자·원문·숫자·본문·proof는 내보내지 않는다. Git pin/install/update/rollback과 카드 흐름은 유지하며 설치는 실행이나 개인 qualification이 아니다. 받는 계정은 자기 자료·새 값·새 proof로 시작한다. 자동 Git push, 외부 전송, 임의 코드 설치는 없다.

D의 자료→초안 준비는 아래의 별도 draft qualification 경로를 따른다. 승인·메일 발송은 replay node가 될 수 없다. browser 로그인/세션/credential 또는 browser grant도 이 구조에 없다.

migration0064는 선행0060–0063 이후 순서대로 실행하고 `user_version`을 갱신한다. 정상 열기와 sealed restore가 같은 경로를 사용한다.

검증: `cargo test --locked -p moosuri-store compound_learning -- --nocapture`는 합성 A/B 채택→새 C 실제 실행/문서→replay 거부, 두 번째 같은 원문 거부, owner 경계, child 예산, 합성 export, 실제 사용 예약의 disable/명시적 재개, owner 편집과 새 proof, 문서 편집 철회를 검사한다. 실제 provider/개인 자료는 사용하지 않는다.

본 단위 검증 결과: 핵심 compound 시나리오1개 통과, 기존 learned 회귀21개 통과 및 schema37 downgrade fixture 보정 후 해당1개 재실행 통과, server cargo check·웹 typecheck 통과, 관련 React4파일25개 통과, verify-plan 통과. 실기기/외부 provider·의미 품질 평가는 하지 않았다.

## 요청한 로컬 메일 초안으로 확장

D의 `PrepareMail`을 마지막 node로 하는 자료→정확 계산→로컬 초안도 별도 allowlist로 검증한다. 원래 맡긴 일에서 Gmail 초안 준비를 요청해야 하며, 원문 query/prefix/scalars와 받는 사람·본문·연결은 매번 현재 소유자 요청/원래 D 권한에서 새로 받는다. recipe에는 계산 구조와 schema/revision만 있고 과거 MailDraftInput·email·resource/binding·credential을 넣지 않는다.

원래 카드의 **검토한 로컬 초안의 수치 결과를 채택해 절차 학습**은 선택적인 개인 학습이다. 원래 D 계약의 실제 MailAction ID/revision/digest, 실제 source/calculation/PrepareMail 영수증과 원문 숫자·정확한 결과가 현재 실제 초안 본문과 최종 결과에 채택됐는지 확인한다. 서로 다른 원문과 현재 입력의 독립된 두 finalized 결과가 필요하며, 채택은 처음 선택한 recipe route에 고정된다. 순수 의미 만족도나 발송 성공을 인증하지 않는다. 초안 내용·받는 사람의 검토와 각 새 MailAction의 최종 발송 허락은 계속 별도다.

새 실행은 learned 절차가 실제 source child를 새로 읽고 계산한 뒤, 기존 D typed `mission_prepare_mail`에 새 현재 MailDraftInput과 실제 계산 receipt를 전달한다. D는 계산 receipt의 exact same-run/current-acceptance 원문 주소를 해석해 실제 원문 identity/revision을 확인한다. 계산 값이나 본문이 같다는 이유로 원문을 추측하지 않는다. 원문 search/read + 각 산술 + 준비 호출은 원래12tool 예산을 공유한다. 1–4개 산술을 포함해7개 node 이하이며 새로운 모델 예산·loop가 없다.

이 node는 **로컬 초안 준비**만 뜻한다. Gmail provider draft 저장, 승인, 발송, Calendar 변경, browser/credential은 재실행 가능한 학습 node가 아니다. 기존 D proposal replay는 원래 action을 반환하고 child 실행/예산을 반복하지 않는다. 기존 recipe·예약 실행·공유 예제는 새 proof가 아니다. 이전 A의 발송 허락을 새 C에 넘기지 않는다. provider 저장/발송의 unknown 결과는 기존 journal에서 멈추며 recipe가 대체 effect를 만들지 않는다.

초안 수정·거절·timing revision 변경과 실패/unknown 발송 정산은 관련 proof를 철회한다. 동일한 원래 초안 revision/digest/content의 정상 `approved → dispatch_committed → sent` 전환은 준비 결과의 qualification을 보존하며 새 proof나 발송 허락을 만들지 않는다. 원문/연결 계정 철회도 기존 run dependency를 통해 proof와 실제 사용한 결과를 철회한다. 실제 pin을 사용한 예약만 `needs_skill`로 멈추며 무관한 예약은 유지한다. owner의 구조 편집은 새 draft recipe revision이며 이전 proof를 상속하지 않는다. 공유는 기존 네 파일/Git/card 경로를 사용하고 새 합성 원문 조건·recipient 예제를 만든다. 개인 trace·원래 수치·수신자·계정·초안·approval/proof는 내보내지 않는다. 받는 계정은 자기 자료/권한과 새 proof로 시작한다.

migration0067도 native63·학습64·작업65·selected browser66 다음에 순서대로 실행한다. 복원은 두 채택 테이블과 성공 proof를 제거하고 파생 결과를 격리한다. 초안 채택 테이블은 원래 run/action identity/revision/digest와 immutable structural route만 연결한다.

초안 확장 검증: 합성 draft-learning2개(여러 철회 경계 포함), 기존 report-learning2개, server cargo check와 웹 typecheck·React27개, verify-plan을 통과했다. 실제 mail/provider 요청이나 개인 자료는 사용하지 않았다. 정상 발송 commitment 중 proof 보존 회귀를 추가해 draft 시나리오3개가 Linux에서 통과했다. 통합 검증은 [별도 기록](reviews/2026-11-15-compound-capabilities.md)을 따른다.
