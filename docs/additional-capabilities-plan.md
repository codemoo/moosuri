# 비서 업무의 추가 구현 계획

2026-11-15, 기준 소스 `07a6b80`. **승인된 구현 계획이다. A/B 구현·검증 완료, C–F 진행 중이며 세부 정본은 [현재 상태](status.md)다.**
[현재 상태](status.md), [전체 P0–P10 계획](plan.md), [기능 계약](features.md)을 유지하면서
P4/P5/P6/P8의 복합 업무·학습·credential 연결을 실제 구현 단위로 좁힌다.
출시 패키지·실계정/실기기 수락·운영 이전은 별도 남은 작업이다.

목표는 한 요청에서 자료를 읽고 산출물을 만들며, 필요한 연결을 기다렸다 이어가고,
실제 완료한 절차를 다음 요청에 재사용하는 것이다. 일반 채팅을 복잡한 업무로 강제하거나
문장마다 새 분류기·에이전트를 만들지 않는다. Rust/React/SQLite, 사용자별 문맥·세션,
기존 실행·예약·외부 영수증을 확장한다. CI는 계속 비활성이다.

## 조사로 확인한 출발점

| 구역 | 이미 있는 것 | 이번에 추가할 것·소스 근거 |
| --- | --- | --- |
| 맡긴 일 | 대화 생성/제어, 일회·반복 읽기, pause/restart, 실제 도구 영수증 | `MissionView.plan`은 최대5개 문자열이다. 단계별 입력·산출물·선행 결과·완료 근거 연결을 추가한다. [core](../crates/moosuri-core/src/missions.rs), [Store](../crates/moosuri-store/src/missions.rs), [runner](../crates/moosuri-agent/src/runner/mission.rs) |
| 업무 산출물 | 일반 대화의 근거 문서와 로컬 메일 초안·승인 발송, 제한된 Calendar 변경 | 미션에는 문서 저장·초안·외부 쓰기가 없다. 기존 서비스에 명시한 미션 권한과 결과 link를 연결한다. [문서](external-reports.md), [메일](mail.md), [Calendar](calendar.md) |
| 개인 학습 | 정확 계산·근거 계산·자료/일본우편/공개 페이지의 고정 모양, 두 성공 뒤 새 입력 재사용 | 임의 혼합 절차는 학습하지 않고 `finalize`는 모든 mission을 제외한다. 지원 연산을 조합한 구조·검증된 산출물의 학습을 추가한다. [학습 Store](../crates/moosuri-store/src/learned_skills.rs) |
| 볼트 | 독립 native broker·live vault·고정 GitHub/Google 조회·최종 승인된 Gmail 발송 | 계정 선택/자동 재사용을 공통화하고 단계 중간의 credential 대기를 추가한다. 현재 `wait_for_credential_tool`은 단독 마지막 도구이며 결과를 모델 호출 없이 끝낸다. [대기 Store](../crates/moosuri-store/src/credential_chat.rs) |
| native 메일 | INBOX 개수·최대5개 미리보기·선택한 plaintext 본문 | 현재 본문은 사용자 표시 전용이다. 검색/thread/첨부와 **별도로 허용한 모델 읽기**를 추가한다. 기존 표시 전용 이력을 모델에 넘기지 않는다. [기존 본문 계약](native-google-message-body.md) |
| 브라우저 | 서버 Chromium·직접 조작·로그인 인계·선택 페이지 관측·제한된 profile 복구 | 임의 agent 클릭/폼·볼트 자동 로그인·browser mission은 없다. 기기 볼트 경계를 보존하는 local browser와 지원 adapter를 별도 단계로 만든다. [현재 workspace](browser-workspaces.md) |
| community | Git pin·카드 설치·update/rollback/remove·일부 합성 export | 새 복합 recipe schema의 검증/import/export와 개인 편집을 기존 흐름에 추가한다. 별도 marketplace나 실행 서버를 만들지 않는다. [community](community-skills.md) |

F24–F27/F29–F37/F42/F58/F63/F66–F69, R14/R16/R20/R23/R25–R28에 연결한다.
숫자 형식·분류기 충돌 수정과22개 재평가 통과는 [이미 완료한 단위](reviews/2026-11-15-live-agent-wave2.md)다.
아래 기능의 구현 증거로 대신 사용하지 않는다.

## 구현 순서와 사용자 결과

| 단위 | 사용자가 할 수 있게 될 일 | 의존성·첫 완료 범위 |
| --- | --- | --- |
| A. 복합 조사와 산출물 | “메일과 저장 자료를 보고 견적 비교 문서를 만들어 줘” | 기존 읽기 미션을 확장해 실제 근거와 문서 ID/revision까지 남긴다. 첫 단위 |
| B. 계정 선택과 이어하기 | “내 업무 계정으로 해줘”; 잠긴 볼트를 열면 원래 단계에서 계속 | A의 단계 identity에 연결. 계정 선택 UI와 descriptor는 A와 독립 개발 가능 |
| C. 볼트 메일을 업무에 사용 | “최근 견적 메일과 첨부를 찾아 비교해 줘” | B 뒤 native 검색·본문/thread, 이어서 선택한 PDF/텍스트 첨부. 기존 표시 전용 기능 유지 |
| D. 실행 가능한 결과 | “회신 초안을 만들고, 지정한 개인 일정을 등록해 줘” | A 뒤 기존 server-managed 서비스부터; native 읽기는 B/C와 결합. 메일 발송은 기존 최종 승인과 실제 영수증 |
| E. 복합 스킬 학습·공유 | 다음 유사 업무에 검증된 절차를 쓰고 기존 제안 카드에서 관리·공유 | A의 읽기/문서부터, D 초안 준비로 확장. C/D의 미완료 외부 기능이 로컬 학습을 막지 않음 |
| F. 인증 브라우저 | 로그인한 페이지를 같은 업무에서 읽고, 지원 사이트에서 계정·TOTP 사용 | B의 binding/wait와 local execution 경계 필요. API 업무 A–E와 분리된 adapter 단위 |

이 A–F는 기존 제품 단계 P 번호를 대체하지 않는다. 큰 단위를 구현·화면·실제 결과까지
연결해 commit/push한다. 하위 체크박스마다 별도 개발 목표나 commit을 만들지 않는다.

## A. 요청에서 근거 문서까지

기존 미션 plan에 버전 있는 단계 계약을 연결한다. 각 단계는 ID, 목적, 필요한 capability,
현재 입력 slot, 선행 단계의 실제 receipt 참조, 산출물, 검사 조건, 상태를 갖는다.
사용자에게 보이는 계획은 기존 최대5개 핵심 단계로 묶고 내부 recipe는 최대8단계로
제한한다. 자유문장 plan만 있는 과거 run은 새 실행 권한을 얻지 않는다.

새 executor/queue를 만들지 않고 `mission_save_plan_at`, 기존 tool slot/child receipt,
`mission_verify_at`, artifact service를 확장한다. 모델은 계획을 제안하며 Store가 현재
요청·실제 연결·입출력 참조를 검증한다. 자료를 읽은 순서만으로 인과관계를 추측하지 않는다.
입력·산출물이 같은 run/owner/acceptance revision에 연결되어야 한다.

첫 수직 구현은 저장 자료/명시한 Gmail/공개 웹2~3개 읽기 → 출처별 주장·누락/충돌 정리 →
지원되는 정확 계산 → 요청한 private 문서 하나 저장이다. 출처의 ID/revision/조회 범위를
보존하고 source/connection/thread 철회가 파생 결과·문서까지 전파되게 한다. 원문 안의
지시는 새로운 작업이 아니다. 사용자가 요청한 문서 저장에 추가 승인창을 붙이지 않는다.

완료 판정은 실제 문서 revision·근거·계산 결과와 비교한다. 정성적 비교의 품질은 별도
검토 상태로 남기며, 파일 하나 저장했다는 사실로 사용자 목적 전체가 달성됐다고 하지 않는다.
같은 요청의 재시작/ACK 유실에도 문서와 완료 기록을 중복 생성하지 않는 것이 종료 조건이다.

## B. 연결을 고르고 중간에서 기다리기

서비스별 capability descriptor에 opaque binding/device ID, 허용 operation/resource,
schema/version, 상태·만료, 사용자가 정한 별칭만 둔다. 볼트 항목·계정 주소를 자동으로
일반 검색 metadata에 복사하지 않는다. 실제 범위의 정본은 신뢰한 기기의 local grant다.
기존 유효 binding이 하나면 자동 재사용하고, 둘 이상이면 선택 카드 한 번으로 지정한다.
명시한 계정이 기본 계정보다 우선한다. 선택은 권한 범위 확대나 다른 기기로의 failover가 아니다.

접수할 때 이미 구현된 operation/schema/version과 실제 probe를 통과한 모델 도구
profile이 있어야 한다. 필요한 연결만 비어 있다면 요청한 operation/resource/계정 선택
범위를 가진 typed 미연결 slot을 미리 고정한다. 연결 완료는 현재 owner/epoch/device/
grant를 검사해 그 slot만 원자적으로 채운다. 전체 profile을 새 설정으로 교체하거나
도구/schema/권한을 추가하지 않는다. 그런 slot이 없는 과거 run은 사용 불가를 정확히
표시하고 연결 후 새 명시적 요청으로 시작한다. 정상 선택/잠금 해제마다 승인을 다시 받지 않는다.

대기 record는 원래 mission/run/step/slot·acceptance revision·선택 UUID·원인·필요한 값·
만료를 저장한다. 안정된 단계 경계에서 lease를 반납하고 worker claim에서 제외한다.
별도 모델로 잠금 상태를 확인하지 않으며 연결/입력 event와 기존 DB loop로 깨운다.
같은 ready event나 응답 재전송은 같은 대기 항목만 처리한다. 아직 해결되지 않은
dispatch가 있으면 일반 대기로 바꾸지 않고 원래 unknown/reconciliation 경로를 따른다.

**시간·용량 계약을 명시적으로 유지한다.** 현재 일반 chat은 첫 dispatch부터120초,
mission은 원래 접수/회차부터 최대하루의 deadline을 가진다. 양쪽 모두6model/12tool,
미션의 최대2repair를 공유한다. 모든 미션에120초가 이미 적용된다고 간주하지 않는다.
대기 만료는 원래 실행·미션·binding·operation 만료 중 가장 이른 시각이다. unlock,
답변, refresh, retry, requeue로 호출 수·시간을 초기화하지 않는다.

같은 유효 run은 새 lease nonce로 이어가며 사용한 결과·호출 수를 보존한다. 만료되면
완료한 유효 산출물과 중단 이유를 남기고 자동 재실행하지 않는다. 필요하면 사용자의
새 명시적 요청으로 이전 결과를 참조하는 새 일을 만들며 새 권한/예산임을 구별한다.
이미 commit한 외부 작업의 늦은 영수증 정산은 모델 재실행과 분리한다.

초기 용량은 사용자별 대기2개와 실행 가능/명시적 정지1개, 설치 전체 미완료16개로
제안한다. 대기도 전체16개에 포함하고 미션 실제 동시 실행1개·일반 채팅 자리 보존은
유지한다. 대기 때문에 같은 사용자의 모든 새 업무를 막지 않는다. request UUID/digest,
현재 revision당 대기1개, expiry/cancel 정리로 중복·무한 적체를 막는다.

연결이 준비됐어도 실행 가능 자리가 차 있으면 ready 상태를 대기 record에 보존한다.
자리가 비면 같은 writer transaction에서 유효한 ready 중 가장 오래된 접수 순서의
하나만 승격한다. 직접 생성·예약 접수·resume은 같은 용량 판정을 사용한다. 자리 반납과
기존 DB due loop에서 다시 확인하므로 ready event를 소비한 채 작업이 사라지지 않는다.
명시적으로 정지한 업무는 사용자가 재개/취소하거나 만료될 때까지 그 자리를 유지하며
자동으로 밀어내지 않는다. ready 대기도 원래 시각에 만료·정리되며 화면 접속이 필요 없다.

UI는 원래 대화/맡긴 일 카드에 “계정 선택”, “볼트 잠금 해제”, “정보 필요”, “만료”와
실제 다음 행동만 표시한다. 새 설정을 마치면 같은 카드로 돌아오며 사용자가 다시
질문을 복사할 필요가 없어야 한다. 비밀번호/TOTP 입력은 신뢰한 기기 화면에서만 받는다.

## C. native Gmail의 검색·본문·첨부

기존 native 미리보기/선택 본문은 계속 표시 전용이다. **모델이 처리하는 Gmail 읽기**는
별도의 capability·최초 연결 grant·실행 profile·도구 결과 타입으로 제공한다. 기존
허용이나 저장된 화면 결과를 소급해 모델 이력·회상·학습·보고서에 넣지 않는다.
새로 허용한 범위는 이후 요청에서 자동 재사용하고 매 본문마다 확인창을 만들지 않는다.

첫 adapter는 고정 Google API의 검색/page cursor, 선택 message/thread의 plaintext다.
이후 같은 선택 identity 아래 PDF/UTF-8 첨부 page를 추가한다. 실제 message ID,
OAuth refresh/access·scope/account 확인은 기기에 두고 서버에는 제한된 opaque selector와
허용된 내용/근거만 반환한다. `MissionReadCredentials`처럼 token을 서버에 주는 기존
interface를 재사용하지 않는다. credential 없는 읽기 request/result 타입과 parsing bounds를
공유하고 native transport와 server-managed transport는 다른 인증 경계를 유지한다.

읽기 결과에는 owner/connection/device/operation·메시지 관측 identity·thread·부분 읽기·
source revision 또는 content digest를 연결한다. 실제 scope가 없거나 account가 다르면
재연결 상태이며 자료 없음·scope 없음·지원하지 않는 포맷을 구별한다. 첨부 처리는
기기의 제한된 parser 경로를 먼저 지원하고 실행 환경이 없으면 명확히 미지원으로 표시한다.

허용된 모델 읽기는 해당 내용의 LLM 전송을 뜻한다. credential 비노출이 모든 메일 본문의
비밀 자동 제거를 뜻하지 않는다. 인증값은 typed projection에서 제외하고 현재 사용한
credential echo는 검사한다. 원문 전체를 정규식으로 가린 뒤 안전하다고 선언하지 않는다.
새 모델 읽기 자료의 문맥/문서/학습 의존성·삭제/철회는 A의 경로를 따르며 기존 표시
전용 기록과 혼합하지 않는다. native 읽기 미션과 반복 읽기는 별도 명시적 background grant,
online 기기와 같은 만료를 요구한다. default-off 연결과 실제 tool probe를 우회하지 않는다.

## D. 초안·승인 발송·개인 Calendar 연결

기존 mail/artifact/Calendar 서비스를 호출하는 좁은 mission operation을 추가한다.
모델에 범용 쓰기·승인 함수를 주지 않는다. 첫 범위는 로컬 새 메일/지원 답장 초안,
기존 provider draft 기능, 현재 명시한 primary solo-event 생성·날짜 변경이다.
읽은 메일에서 값이 나오는 경우 operation의 권한은 현재 사용자 요청에, 값은 실제
읽은 근거와 명시한 선택 조건에 묶는다. 불명확한 수신인·중복 대상·충돌한 날짜만 질문한다.

준비 단계는 stable UUID와 digest로 실제 draft/action revision을 만들고 미션 단계와
원자적으로 연결한다. “초안 만들기”는 초안 저장이 완료이고 “보내기”는 승인 대기다.
**메일은 매번 최종 계정/수신인/제목/본문/첨부 여부/시각을 보여주고 허락받는다.**
편집하면 이전 승인이 무효다. 기존 `MailAction`·native send journal·고정 예약 발송을
사용하고 새 발송 원장을 만들지 않는다. native sender의 현재 단일 plaintext 범위와
server-managed sender의 지원 reply/thread 범위를 서로 있다고 가정하지 않는다.

미션 추론 deadline이 지나도 이미 준비한 초안은 열 수 있다. 나중의 최종 발송 승인은
그때 검증한 정확한 MailAction만 허용하며 만료된 미션 모델을 되살리지 않는다.
발송 count/실제 receipt를 읽어 원래 카드의 상태를 갱신한다. provider 접수와 수신자
배달을 구별하고 ACK 유실은 원래 action 조회로 정산한다. 자동 재발송·다른 기기 재전송은 없다.
취소 전 commit 여부, 기존5분 scheduled-send 지연 한도, restore 격리를 그대로 따른다.

Calendar는 기존 owner/primary/solo-event·ETag·UUID/receipt 경로로 실행하고 명확한
현재 요청에 새로운 승인 팝업을 붙이지 않는다. native Calendar가 필요하면 별도
읽기/쓰기 adapter와 local grant를 구현해야 하며 현재 server 연결을 vault-native로 부르지 않는다.
반복 업무는 읽기/문서/초안 준비부터 확장한다. 미래 생성 메일 자동 발송·반복 Calendar
쓰기·참석자 초대는 이번 범위에 넣지 않는다. 기존 고정 내용 예약 발송은 유지한다.

## E. 실제 성공 절차에서 복합 스킬 만들기

먼저 A의 자료→계산→문서, 이어 D의 자료→초안 준비를 대상으로 한다. 사용 가능한
typed 도구≤9종, recipe≤8단계의 순환 없는 구조에 현재 입력 slot·실제 결과 참조·
정확한 pin/schema·검증 조건을 저장한다. 모든 leaf 호출을 기존12tool 예산에서 차감하고
recipe 안에 새 model/tool 예산이나 무한 반복을 만들지 않는다. 지원되지 않는 adapter를
모델이 생성·설치하는 기능은 없다. 새 adapter는 contributor의 Rust 구현·OpenSpec·
검증을 거친다. 스킬은 구현된 도구를 조합하는 절차다.

실제 서로 다른 입력/자료의 finalized 결과2개에서 독립 receipt·산출물 adoption을
확인한 구조만 사용 가능해진다. 기존 recipe 재실행, 같은 입력 replay, imported 예제,
모델의 성공 선언은 새 proof가 아니다. 미션 학습은 현재의 전체 제외를 제거하는 식으로
열지 않고 지원되는 node/결과에 한정한 새 qualification 경로를 만든다.

기계 검증(원문 identity·계산·문서/draft revision·실행 결과)과 의미 품질을 분리한다.
주관적 요약/비교의 만족도를 기계 성공2회로 인증하지 않는다. 해당 결과는 검토 필요를
유지하고 사용자 수정/피드백은 별도 품질 정보다. 신뢰할 만한 outcome predicate가
없는 새 모양은 후보/일반 지침으로 남고 자동 실행 recipe로 승격하지 않는다.
메일 승인·발송 단계는 replay 가능한 학습 node가 될 수 없다. 준비 recipe가 새로 만든
초안마다 D의 기존 승인 흐름이 새로 적용된다.

새 C 요청에서 새 계정·날짜·사람·원문·숫자로 다시 읽고 수행해야 한다. A/B의 값·
resource/binding ID·token·본문은 recipe에 담지 않는다. 원문/qualification 삭제 시
proof를 철회하고, 실제 사용 pin에 따라 관련 결과/예약만 정지한다. owner 편집은
새 revision을 만들고 기존 proof를 물려받지 않는다. 기존64route/tombstone 한도,
실패 귀속/격리, 삭제 후 자동 재생성 방지, 예약의 명시적 재개 정책을 유지한다.

기존 “제안/할 수 있는 일”에서 사용 가능·연결 필요·검증 중·중단을 보여주고 실행·
편집·숨김·disable·remove를 제공한다. 일반 채팅마다 스킬 목록이나 자료 개수를 붙이지 않는다.
community export는 허용 구조와 새 합성 예제로 패키지를 재구성한다. 원래 trace를
복사하지 않는다. Git pin/install/update/rollback 경로를 유지하고 수신자는 자기 계정과
새 proof로 시작한다. 자동 Git push나 다른 사람에게 전송하는 기능은 추가하지 않는다.

## F. 인증 브라우저의 단계적 확장

첫 사용자 결과는 **직접 로그인한 뒤 선택한 페이지 읽기를 원래 업무에 연결**하는 것이다.
현재 서버 workspace의 인계는 유지하고 사용자의 명시적 선택/새 관측에서만 이어간다.
이 경로는 서버가 관리하는 쿠키/브라우저라는 경계를 표시한다.

볼트 password/TOTP를 사용하는 자동화는 별도 **신뢰한 기기의 local browser+native broker**
경로로 만든다. 운영자가 관리하는 Linux Chromium에 해당 비밀이나 cookie를 주면서
관리자가 볼 수 없다고 주장하지 않는다. local browser 지원·journal·세션 수명·제한된
관측 projection을 먼저 구현하고, 한 사이트/정확한 읽기 업무의 adapter로 수락한다.
첫 실제 사이트는 해당 adapter 착수 시 공개 로그인 방식·공식 API 대안·정책·테스트 계정과
완료 근거를 조사해 고정한다. 아직 특정 사이트의 자동 로그인을 지원한다고 약속하지 않는다.

지원 adapter만 고정 origin/field/challenge에 입력하며 비밀번호/TOTP seed·code·cookie·
CDP·임의 JS·form value를 모델/서버 DB/공유 스킬로 반환하지 않는다. DOM·URL·오류·
iframe의 secret echo를 검사한 제한된 결과만 내보낸다. passkey/hardware MFA는 필요한
사용자 gesture로 넘긴다. 현재 원래8시간 workspace/30분 idle·인계5분의 수명과 회복 시
빈 화면 확인을 유지하며 기다렸다는 이유로 세션을 연장하지 않는다.

자동 폼 제출은 사이트별 typed action·확정 시점·조회 가능한 결과가 있어야 별도
구현 단위가 된다. 모호한 click/POST 결과는 재시도하지 않는다. 결제/주문/취소·전화·
정부 서류 제출·범용 사이트 로그인은 이번 A–E 종료 조건이 아니며 기능표의 후속을 유지한다.

## 구현과 함께 닫을 검증

각 단위는 실제 Store/API/Runtime/React/native 경로에서 사용자 결과를 확인한다.
다음 실패 원인을 대표하는 소수의 통합 시나리오를 선택하고 기존 테스트를 통째로 옮기지 않는다.

| 시나리오 | 확인할 결과 |
| --- | --- |
| 같은 제목의 두 계정 자료·충돌하는 견적 | 요청 계정의 실제 자료·조회 범위/누락·정확 계산·문서 link. 모호한 값을 꾸미지 않음 |
| 문서/초안 저장 직후 ACK 유실·재시작 | 같은 UUID/산출물1개, 완료 단계 재실행0, 호출 수/예산 보존 |
| 업무 중 볼트 offline→unlock | worker 자리 반환, 다른 대화/허용한 다른 업무 진행, 같은 미완료 단계 재개 |
| 두 대기 작업 동시 ready·다른 작업이 자리 점유 | ready 보존, 자리 해제 후 하나씩 공정 승격, 중복 접수/깨우기 유실0, 원래 만료 시 화면 없이 정리 |
| 기다리는 중 만료·수정·철회·계정 전환 | 오래된 응답은 새 계획을 완료하지 않음. 만료 시 자동 재개0, 실제 in-flight 영수증만 정산 |
| 같은 서비스 여러 계정·기기 | 최초 정확한 선택과 이후 자동 재사용; 다른 계정/기기로 암묵적 변경0 |
| 실행 중 연결·도구 설정 추가 | 원래 지원/probe한 미연결 slot만 채움, profile 전체 교체/새 도구 권한 추가0 |
| native 화면 전용과 모델 읽기 | 옛 preview/body의 모델/회상/학습 유입0; 새 grant의 제한된 내용/근거만 전달 |
| 메일 초안→편집→최종 허락→응답 유실 | 허락 전 발송0, 오래된 허락 무효, 같은 action의 POST≤1, unknown을 성공으로 말하지 않음 |
| source에서 얻은 일정·대상 충돌 | 요청한 operation만 실행, 실제 인용/날짜·ETag/receipt, 불명 대상 자동 선택0 |
| 독립 A/B→새 C 학습, owner 편집·삭제 | 실제 새 입력/원문 재조회, 재사용을 학습으로 재계수0, proof/파생 결과 철회 |
| 예약/스킬 disable·다음 due·resume | 관련 예약만 정지, 지난 회차 재생0, 새 회차 fresh receipt, 매번 생성한 메일 자동 발송0 |
| community 다른 사용자 설치 | private canary/export0, credential/성공 proof/승인 승계0, 원래 Git pin/rollback 유지 |
| local browser 인계·MFA·오류·만료 | 비밀 echo/모델 노출0, 수명 연장0, 불명 제출 재실행0, 지원 범위 정확 표시 |

실모델 품질 평가는 구현을 고친 사례와 새 독립 사례를 분리하고 처음 보는 입력에서
실제 결과·근거·모호함 처리·recipe 유무의 성공/호출 수/지연을 비교한다. 누락 사용량은
unknown으로 남긴다. 특정 모델 이름으로 tool capability를 추정하지 않으며 공유 API/local,
개인 ChatGPT의 해당 profile probe를 각각 확인한다. 현재 Claude text-only 연결에 새 도구를
자동 제공하지 않는다. 모델/effort 기본값을 낮추지 않는다.
일반 채팅의 새 분류 호출0·대기 중 모델 polling0을 유지한다. 검증을 통과한 경로는
새 변경·실패·위험이 생길 때만 다시 실행한다. 실제 provider/device 수락은 별도 표시한다.

## 변경 관리와 착수점

[새 통합 change](../openspec/changes/extend-compound-assistant-work/tasks.md)가 A–F를 추적한다.
기존 [mission](../openspec/changes/build-owner-missions/tasks.md),
[credential](../openspec/changes/connect-vault-credentials/tasks.md),
[학습](../openspec/changes/learn-private-task-routes/tasks.md)의 미완료 상위 요구는 닫지 않는다.
동일 기능에 별도 queue/DB/spec 저장소나 경쟁하는 상태 문서를 추가하지 않는다.

첫 구현 commit은 **A의 자료 조사→근거 문서 저장**이다. 범위는 core 단계/산출물 DTO,
Store의 idempotent 기록/근거/복원, 기존 runner의 지원 operation, React 원래 작업 카드와
문서 이동까지다. B의 descriptor/선택 UI는 별도 파일 소유권으로 진행할 수 있다.
각 단위의 schema 변경은 normal-open/backup/restore/삭제 journal까지 함께 다룬다.
서명된 native protocol은 discriminant/기존 wire bytes를 보존하며 호환 version을 명시한다.
외부 작업·인증·저장 경계는 독립 검토 후 관련 검사·OpenSpec·비밀 탐지·상태 갱신을
거쳐 commit/push한다. 구현되지 않은 단계는 체크하지 않고 archive하지 않는다.

계획 중 발견한 문서 불일치도 좁게 바로잡는다. 대화 미션 생성과 private recipe 재사용은
이미 구현됐다. 일반 chat120초와 mission최대하루의 구분도 현재 코드에 맞춰 쓴다.
이 문서 정리는 새로운 runtime 검증이나 기능 구현으로 세지 않는다.

## 참고한 설계 근거

Muse를 주 기준으로 한 [기존 공개 사례/구조 조사](research/personal-agent-tasks-2026-11-13.md)와
[브라우저 조사](research/browser-workspaces-2026-11-13.md)를 사용했다. 비공개 구현이나
홍보 성공률을 무수리의 검증 결과로 취급하지 않는다. 아래 세 원문은2026-11-15 다시 열람했다.

- [Anthropic, Building effective agents](https://www.anthropic.com/research/building-effective-agents): 단순한 도구 loop, 환경의 실제 결과, 필요한 경우에만 workflow 복잡도 추가.
- [Anthropic, Effective harnesses for long-running agents](https://www.anthropic.com/research/effective-harnesses-for-long-running-agents): 명시한 완료 항목·영속 진행·다음 실행이 이해할 수 있는 증거. 코딩 에이전트의 구체 구현을 비서에 그대로 이식하지 않음.
- [Agent Skills specification](https://agentskills.io/specification): 작은 discovery metadata, 필요할 때 읽는 본문, Git에서 관리할 수 있는 패키지. 표준의 선택적 executable script는 무수리의 임의 코드 실행 허용이 아님.
