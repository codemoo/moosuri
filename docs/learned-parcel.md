# 일본우편 조회에서 배운 개인 절차

기존 [Japan Post 최근 요약 조회](parcel-tracking.md)의 실제 typed 결과가 최종 답변에 채택되면,
그 조회 방법을 [개인 학습 절차](learned-skills.md)로 재사용할 수 있다. 배송사·운송장·상태를
기억해 다시 보여 주는 기능은 아니다. Japan Post 하나의 최근 요약 조회에 한하며 전체 이력,
다른 배송사, 예상 도착, 배송 알림/구독, 백그라운드 실행과 외부 쓰기는 지원하지 않는다.

## 두 개의 실제 독립 예제

같은 소유자의 서로 다른 finalized run에서 서로 다른 현재 운송장 번호를 실제 조회해야 한다.
첫 canonical 번호는 candidate이며 두 번째 독립 번호가 있으면 usable이다. 하이픈/소문자만
다른 같은 번호, 같은 slot replay, 다른 계정의 조회, 미완료/취소된 run은 독립 예제가 아니다.

Store는 단순한 success flag나 `완료` 문장을 proof로 보지 않는다. 실제 완료된 external dispatch,
서버가 기록한 원래 request/run의 현재 번호 provenance, 정확한 공식 URL과 실제 관측 시각,
`state=observed`인 완전한 최근 요약을 확인한다. 최종 답변은 그 정확한 번호·보고 날짜·상태와
공식 URL을 사용해야 한다. 최종 완료 model step의 답변에서 확인하며, 앞선 도구 round의 문장이나 commentary만 남긴 결과는 proof가 아니다. `not_found`,
`provider_changed`, 네트워크 실패나 과거 번호를 재사용한 후속 조회는 승격 근거가 되지 않는다.
이는 좁은 조회 결과가 답변에 쓰였다는 증거이며 일반적인 의미의 업무 성공이나 배송 완료를
증명하지 않는다.

route에는 `japan-post-latest-v1` 구조만 저장한다. proof에는 기존 private 영수증/run link와
canonical 입력/영수증 digest만 보관하며, route·discovery metadata에 번호·상태·계정·과거
조회 결과를 복사하지 않는다. 기존 toolset generation2와 계산/자료 조회 pin을 유지한다.
이 추가로 다른 학습 절차의 proof를 일괄 무효화하거나 DB 스키마를 변경하지 않는다.

## 새 요청으로 실행하기

usable 절차는 기존 제안/할 수 있는 일 화면과 새 대화 discovery에 **일본우편 최근 배송 상태 조회**로
표시된다. `learned_skill_execute`는 그 정확한 pin과 `{kind:parcel,tracking_number:...}`를 받는다.
사용자는 현재 요청에 Japan Post와 유일한 번호를 명시해야 한다. 입력 번호가 현재 요청의
canonical 번호와 다르거나 빠졌거나 여러 번호가 있으면 조회하지 않고 번호를 확인한다.
모델·일반 이력·학습 예제·과거 상태에서 새 번호나 성공을 만들 수 없다.

실행할 때마다 현재 공개 읽기 설정·기존 parcel-tracking pin·소유자/세션/모델/run fence를
검사하고 기존 `parcel_track`의 고정 목적지/parser로 새 IO를 한다. 실제 새 관측과 URL/시각을
반환하며 결과가 없거나 provider 형태가 바뀌면 그대로 드러낸다. 일반 `parcel_track`의 같은
대화 번호 후속 조회 계약은 유지하지만 학습 절차 자체는 그 fallback을 사용하지 않는다.

한 번의 조회는 기존 도구 예산 한 개를 사용한다. 부모 slot과 기존 external dispatch nonce가
결과 유실·재시도를 제어하며, 완료된 같은 slot은 영수증을 반환하고 IO/예산을 중복 사용하지 않는다.
실제 요청/결과는 부모에 연결된 private child receipt 한 개로 남는다. 현재 번호/권한이 부족한 거절이나 provider 실패·부재·형태 변경은 절차 자체의 실패 횟수를 늘리지 않는다. 결과 불명은 기존 dispatch
계약대로 남고 자동 재전송하지 않는다. 미션에는 private learned route 실행 권한이 없다.

기존 숨기기·비활성화·삭제와 owner isolation을 사용한다. 학습 예제 대화를 삭제하면 그 proof가
제거되고 독립 예제가 하나만 남으면 candidate로 돌아간다. 실제 사용/저장 호출에 연결된 파생
답변은 기존 철회 경로로 제거한다. 절차 삭제는 tombstone을 남기므로 새 조회로 자동 부활하지 않는다.
공개 읽기를 끄면 새 IO와 늦은 private 결과를 허용하지 않는다.

## 절차 공유하기

기존 제안 화면의 **절차 공유하기**는 알려진 Japan Post route로부터 네 파일을 새로 만든다.
현재 요청 번호를 요구하고 `parcel_track`의 실제 최근 요약을 사용하라는 구조만 내보내며,
리포지터리가 제공하는 새 합성 번호/예제와 Apache-2.0 라이선스를 포함한다. 과거 번호·상태·
원문·계정·credential·trace·성공 횟수·실행 권한은 export 입력이 아니다. 설치/미리보기만으로
합성 번호를 실제 조회하지 않으며 자동 게시/전송하지 않는다. 다른 사용자는 자기 현재 요청과
설정으로 도구를 실행하고 원래 소유자의 proof나 자동 실행 권한을 얻지 않는다.

검증은 합성 HTML→실제 loopback HTTP/parser→typed 외부 ledger→최종 답변→candidate/usable→
새 번호 IO와 철회/export canary를 다룬다. 실제 사용자 번호·일본우편 positive 응답·실계정·
넓은 모델 언어 이해도·실서비스 안정성은 검증하지 않았으며 이를 완료로 주장하지 않는다.
