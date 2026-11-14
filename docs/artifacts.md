# 개인 문서 · 목표 · 요청 기록

`/app/artifacts`에서 계정 소유 문서·목표·요청/약속을 만들고 편집한다. kind 탭,
버전 기록, Markdown 및 인쇄용 HTML 내려받기를 제공한다. 로컬 저장에 승인 모달은 없다.
이 기록은 메일 발송·미래 모니터링·알림 예약의 실행 증거가 아니다.

## 현재 경계

- 제목200 UTF-8바이트, 본문32KiB, 폐쇄형 payload8KiB, 직접 reference12개.
  계정별256개 identity(삭제 포함), identity별128개 편집 version.
- 문서는 Markdown 텍스트다. 미리보기와 HTML export는 리터럴 텍스트를 escape한
  `<pre>`이며 원격 이미지·스크립트·임의 링크를 실행하지 않는다. HTML 응답에는
  `default-src 'none'`와 sandbox CSP, 다운로드 헤더가 있다. private cache는 없다.
- 목표는 outcome/success criteria/progress/lifecycle를 보관한다. 새 채팅 목표는 active,
  lifecycle의 변경은 명시적 편집기에서 한다. 계획 작성만으로 달성으로 바꾸지 않는다.
- followup은 request/promise/waiting_for_reply, requested/committed/waiting/
  resolved/cancelled, none/partial/delegated, 독립적인 mute/due/review/snooze를 가진다.
  UI는 기기 시간대를 표시하고 현지 날짜·시간을 입력받아 UTC로 저장한다. 채팅의 새 날짜는 현재 요청의 정확한
  `UTC:<Unix seconds>` 표기만 허용한다. 모호한 자연어 날짜를 추정하지 않는다.
- briefing은 삭제·철회·resolved/cancelled·mute·현재 snooze 기록을 제외하고 최대5개를
  review 우선, 없으면 due, 갱신 역순으로 반환한다. continuation과 coverage를 제공한다.
  cursor는 generation/kind/briefing/time에 묶이며5분 만료한다. 알림 scheduler는 없다.

## 실제 채팅 저장

새로 admission한 tool-enabled shared 또는 개인 ChatGPT run은 별도 schema1 artifact profile을 고정한다. 개인 연결도 별도의 쓰기 승인 모달이나 probe 없이 기존 정확한 tools proof와 현재 명시적 요청을 사용한다.
과거 RunToolProfile과 이미 존재한 run에 capability를 추가하지 않는다. 현재 요청의
문서/보고서/목표/요청 기록 작성·저장·수정 cue 또는 사용자 선택 target이 있으면,
**현재 user message만** 별도 모델 단계로 분류한다. 원문·이전 대화·skill body는 그
단계에 전달하지 않는다. 읽기 요청은 write grant 없이 artifact read/search/briefing만
사용한다. 분류 품질은 실모델 평가되지 않았으며 scripted fixture는 의미 정확도 점수가 아니다.

Store가 literal intent quote, 현재 계정·lease·profile, tool 실행 이전 상태를 재검증하고
create/edit kind와 exact target/head를 봉인한다. 기존 target은 composer의 trusted 선택,
또는 현재 요청의 정확한 제목과 **계정 전체 최대256개 후보 중 유일한 exact match**다.
동명/누락/불명확 대상은 고정 clarification으로 끝난다. 모델에게 대상 ID를 고르게 하거나
첫 후보를 택하지 않는다. 이 v1에는 의미 기반 후보 선택 모델이 없다. 이후 추가한다면
sealed candidate snapshot에 대한 index-only 선택이어야 한다.

`artifact_write`는 봉인한 한 대상만 변경하며 모델이 tenant/id/request UUID를 정하지 않는다.
초기 body는 최대2048바이트, payload1024바이트, title200바이트이고 **실제 직렬화된 전체
slot은 기존4096바이트**를 초과할 수 없다. 큰 인용/escape가 envelope를 넘으면 거절한다.
수정은 읽은 pinned version의 유일한 literal anchor(최대512바이트)를 replacement body로
바꾼다. fuzzy edit/전체 교체/자동 CAS repair는 없다. 큰 수정은 편집기에서 한다.
한 run은 하나만 저장하며 mutation tool은 단독 slot으로 실행한다. 여러 record batch는
이 v1에서 거절한다. 저장 후 추가 모델을 호출하지 않고 실제 DB receipt로 고정 확인문과
버전 링크를 반환한다. 저장 모델의 미확정 preview는 사용자 확인문으로 전달하지 않는다.

v1 채팅 저장은 source/memory/artifact provenance만 지원한다. 메일·웹·브라우저 직접
읽기 이력이 있는 대화의 저장은 외부 근거 철회 연결을 구현하기 전까지 명시적으로 거절한다.
해당 write run에서는 외부 읽기 도구도 노출하지 않는다. 필요한 내용을 먼저 자료로
수집하거나 새 대화에서 현재 요청과 저장된 자료로 작성한다. 이를 외부 근거 지원으로 표시하지 않는다.

## 수정·철회·복구

HTTP도 동일한 Store transaction helper를 사용한다. 요청 UUID+정확한 payload digest와
receipt가 version과 함께 commit된다. 같은 요청은 같은 content-free receipt를 돌려주고
같은 UUID의 다른 payload는 충돌한다. editor는 network ACK 불명 때 원래 UUID/payload를
유지해 재확인하며,409에서는 로컬 초안을 보존하고 서버 현재 내용을 별도로 연다.

읽은 source/memory/artifact dependency는 결과 전달 전에 등록된다. derived write는 모델이
citation을 생략해도 run이 읽은 전체 dependency를 물려받는다. Memory의 source evidence와
artifact의 전이 dependency도 포함하며 closure256개를 넘으면 거절한다. editor 수정은 기존
dependency를 유지한다. 새 독립 작성 object만 빈 dependency로 시작한다. 화면은 저장된
근거의 최대12개 링크를 보여 주며 전체 전이 closure는 계속 철회 대상으로 보관한다.

source 교체/삭제, memory 정정/forget, parent artifact 변경/삭제는 같은 normal/replay
invalidation 경로를 통해 dependent title/body/payload/history와 run output/tool/checkpoint를
철회한다. 이전 revision으로 되살리지 않는다. 독립 작성 content는 남는다. 외부 다운로드나
사용자 로컬 사본을 원격 삭제한다는 보장은 없다. 자동 기억 grant는 artifact write 권한이 아니다.

삭제는 기존 successor destructive-intent journal에 opaque DeleteArtifact intent로 추가한다.
append/fsync가 adoption이며 DB effect/receipt/watermark가 함께 commit한다. source/memory
정상 경로와 replay가 같은 hook을 탄다. conversation 삭제는 saved artifact를 삭제하거나
재생성하지 않는다. 기존 backup은 authoritative SQLite의 artifact tables도 포함한다.

## 저장 형식과 검증

SQLite migration16은 자동 기억14, 삭제 의도15 다음에 적용한다. 문서·목표·요청,
버전·근거·저장 영수증은 SQLite 안에 보관하며 별도 원문 파일은 만들지 않는다.
삭제 journal의 검증 기준은15로 유지한다. 기존 기억 추출·망각 억제와도 함께 동작한다.
추가 Rust/웹 dependency는 없다.

실제 통과 범위와 남은 검증은 [검증 기록](reviews/2026-11-13-personal-artifacts.md)에 있다.
합성 응답 검사는 실모델의 의미 정확도나 미션 성공률을 대신하지 않는다.

개인 ChatGPT의 artifact classifier도 현재 user message만 받아 plan namespace와 정확한 model/effort로 실행한다. 각 분류 전 기존 RunLease와 token refresh CAS로 연결을 재확인한다. 이미 planned read-only인 profile은 쓰기 grant로 승격하지 않는다. 실제 영수증을 commit한 뒤 모델을 다시 호출하지 않으며, 개인 연결 해제·원문 철회·revision 충돌은 기존 Store 경계로 늦은 저장을 막는다. Calendar 변경·background 미션·첨부는 이 기능으로 활성화되지 않는다.
