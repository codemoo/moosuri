# 개인 문서 · 목표 · 요청 기록

`/app/artifacts`에서 계정 소유 문서·목표·요청/약속을 만들고 편집한다. kind 탭,
버전 기록, Markdown 및 인쇄용 HTML 내려받기를 제공한다. 로컬 저장에 승인 모달은 없다.
요청·약속의 미래 날짜는 아래 정책으로 로컬 알림을 예약한다. 기록 자체는 메일 발송이나 미래 모니터링의 실행 증거가 아니다.

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
  cursor는 generation/kind/briefing/time에 묶이며5분 만료한다. 날짜 알림은 별도의 아래 정책을 따른다.

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

채팅의 문서 저장은 source/memory/artifact와 완료된 공개 HTTP·자기 Gmail·기본 Calendar
읽기의 실제 근거를 연결한다. 현재 요청의 읽기와 같은 대화에서 선택된 이전 읽기를
문서 하나로 저장·수정할 수 있다. 브라우저·native 미리보기·Calendar 변경 같은 지원하지
않는 외부 문맥은 저장하지 않는다. 목표/요청·약속의 직접 외부 읽기 저장 권한을 넓히지
않는다. [외부 자료 보고서](external-reports.md)의 지원 범위와 철회 동작을 따른다.

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
정상 경로와 replay가 같은 hook을 탄다. 독립 작성한 saved artifact는 대화 삭제로
삭제되거나 재생성되지 않는다. 외부 읽기에 근거한 문서는 원래 대화 삭제 시 함께 철회한다.
기존 backup은 authoritative SQLite의 artifact tables도 포함하며, 복원은 외부 근거 문서를
보수적으로 철회한다.

## 저장 형식과 검증

SQLite migration16은 자동 기억14, 삭제 의도15 다음에 적용한다. 문서·목표·요청,
버전·근거·저장 영수증은 SQLite 안에 보관하며 별도 원문 파일은 만들지 않는다.
삭제 journal의 검증 기준은15로 유지한다. 기존 기억 추출·망각 억제와도 함께 동작한다.
추가 Rust/웹 dependency는 없다.

실제 통과 범위와 남은 검증은 [검증 기록](reviews/2026-11-13-personal-artifacts.md)에 있다.
합성 응답 검사는 실모델의 의미 정확도나 미션 성공률을 대신하지 않는다.

개인 ChatGPT의 artifact classifier도 현재 user message만 받아 plan namespace와 정확한 model/effort로 실행한다. 각 분류 전 기존 RunLease와 token refresh CAS로 연결을 재확인한다. 이미 planned read-only인 profile은 쓰기 grant로 승격하지 않는다. 실제 영수증을 commit한 뒤 모델을 다시 호출하지 않으며, 개인 연결 해제·원문 철회·revision 충돌은 기존 Store 경계로 늦은 저장을 막는다. Calendar 변경·background 미션·첨부는 이 기능으로 활성화되지 않는다.

## 요청·약속의 날짜 알림 (F28/F32)

명시적으로 저장한 active followup은 미래 기한과 재검토 날짜를 각각 한 번 예약한다.
동일 시각은 하나로 합친다. 미래 미루기 날짜를 지정하면 그 날짜 이전의 기한·재검토는
미루기 알림 하나로 모으고, 이후 날짜는 유지한다. 미루기만 지정해도 그 시각에 한 번
알린다. 이미 지난 날짜는 브리핑에만 남고 자동 소급 발송하지 않는다. 지난 기한을
다시 알리고 싶으면 미래 미루기 또는 새 미래 날짜를 저장한다. 저장한 2100-01-01 UTC
이후 날짜는 기존 scheduler의 시각 한계 때문에 기록·브리핑만 제공한다.

schema0046 이전의 기록은 자동으로 예약하지 않는다. 현재 기록을 명시적으로 다시
저장하면 그 시점의 미래 날짜부터 예약한다. 저장 화면의 **저장된 후속 알림**은 서버가
실제로 저장한 버전·종류·시각·상태이며, 편집 중인 날짜를 예약됐다고 표시하지 않는다.
소식함에는 원래 기록의 링크·버전·종류·예정 시각이 있다. 링크는 현재 유효한 소유 기록에만
노출한다. 완료·취소·기록의 숨김은 새 알림을 만들지 않는다.

별도 실행 framework 없이 기존 5초 reminder loop, quiet hours, 소식함과 opt-in Push를
사용한다. 반복·LLM 호출·외부 메시지 발송은 없다. 서버가 꺼져 여러 날짜를 지나면 해당
버전의 가장 최근 도래 날짜 하나만 전달하고 앞선 날짜를 coalesce한다. 같은 저장 요청의
재시도와 재시작은 중복 소식을 만들지 않고, 이미 전달한 동일 시각은 시계가 뒤로 가도
편집만으로 다시 알리지 않는다. 계정별 기존256 identity/128 version 한계가 예약 이력을
제한한다. 내부 예약은 일반 예약 목록·대화 제어·64개 일반 예약 한도에 포함하지 않는다.

기록 수정·날짜 변경·미루기·완료·취소·숨김·삭제와 근거의 전이 철회는 같은 Store transaction에서
이전 pending occurrence와 Push를 차단한다. 기존 소식의 내용은 지우고 숨김 처리하며,
유효 기록 링크도 제거한다. Push가 이미 dispatch commit된 경우 실제 결과는 보존하지만
재시도·passive display 권한은 영구 차단한다. 이미 외부 기기에 표시된 알림의 회수는 보장하지 않는다.
소식함의 source mute는 같은 기록의 후속 날짜·버전에 유지되며, 다시 켜도 과거 소식을 재생하지 않는다.
복원은 기존 scheduler/Push quarantine을 적용해 과거 알림을 자동 재개하지 않는다. 소유자가
기록을 다시 저장하면 미래 날짜부터 예약한다.

[합성 검증과 한계](reviews/2026-11-14-followup-notifications.md)를 참조한다.
