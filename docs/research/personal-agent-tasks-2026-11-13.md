# 개인 비서가 대신 수행하는 업무: 사례와 구현 방향

2026-11-13 공개 자료를 다시 확인했다. Muse가 주 기준이며 Pickle은 국내 업무,
Lindy는 메일·일정 절차, Manus는 조사 산출물의 보조 기준이다. 아래는 공개 문서에
나온 사례와 무수리의 구현 제안이다. 제품에 로그인해 실제 구매·예약·발송을 실행한
벤치마크가 아니며, 홍보 문구나 앱 평점을 성공률로 사용하지 않는다.

## 어떤 일을 맡기는가

입력 예시는 무수리용으로 새로 작성했다. 필요한 도구와 완료 증거는 우리의 설계이며
해당 서비스의 비공개 구현을 추정한 설명이 아니다.

| 사례 | 공개 근거와 그 수준 | 무수리에 필요한 동작·완료 증거 |
| --- | --- | --- |
| 학교 공지에서 준비물·신청 마감 챙기기 | Muse 디자이너가 학교 메일·웹, 가족 일정, 준비물 장바구니와 마감 알림 경험을 서술 [M1] | 메일/웹 읽기→기한·담당자 정리→사용자가 요청한 일정/알림. 날짜 원문과 생성한 일정 식별자 |
| 회의 전에 결정사항·미해결 질문 정리 | Muse의 개인 문맥·문서 작업을 바탕으로 한 **무수리 파생 업무** [M1] | 관련 원문과 유효한 기억 확인→바뀐 결정 구별→짧은 회의 준비문. 예약 완료와 구별 |
| 저장한 레시피로 장보기 목록·저녁 모임 준비 | Muse 공식 출시 사례, 친구 식이 제한도 활용 [M2] | 저장 콘텐츠·취향 읽기→인원별 목록→필요하면 장바구니/초대. 목록 작성과 구매/발송 구별 |
| 여행 계획부터 예약까지 맡기기 | Muse는 여행 예약을 공식 기능으로 설명 [M2] | 날짜·동행자·총예산→가능 옵션·총액·조건→예약→상태 확인. 공급자의 확인번호 |
| 상품 조사·가격 비교 | Meta의 싱가포르 출시 설명에 쇼핑몰 가격 비교 사례 [M3] | 같은 옵션·수량·배송비·세금·통화로 비교. 조회시각과 원문, 정확한 합계 |
| 목표를 주간 계획으로 만들고 계속 조정 | Meta AI/Muse Spark의 마라톤 훈련 계획·월요일 공유 사례 [M4] | 목표·제약→계획→주기 실행→변화만 알림. 생성한 실행 규칙과 실제 회차 기록 |
| 매일 아침 메일·일정·결정할 일 알려주기 | Pickle 예약 작업 사용 문서 [P3] | 계정·시간대·중요 기준→원문 기반 브리핑→예약/알림. 누락된 연결과 실행 실패도 표시 |
| 뉴스레터 보관, 답장할 메일 남기기 | Lindy 메일 분류 사용 문서 [L1] | 계정별 규칙→분류→라벨/보관. 실제 message ID와 변경 상태; 보관을 삭제로 확대하지 않음 |
| 답장 없는 메일 후속 초안 만들기 | Lindy는 정한 기간 뒤 후속 초안을 저장한다고 설명 [L2] | 원래 thread·마지막 발신/수신·제외 조건→초안. draft ID; 초안을 발송했다고 말하지 않음 |
| 메일을 읽고 다음 주 회의 잡기 | Lindy는 CC와 일정 의도, 빈 시간 제안·예약을 문서화 [L3] | 참석자·시간대·길이·버퍼→일정 조회→초대. event ID와 발송 상태; 참석 수락과 구별 |
| 식당에 전화해서 예약·변경하기 | Pickle 작업 문서와 공식 앱 설명 [P1][P5] | 가게·시간·인원·대체 허용 범위→통화→조건 재확인. 통화 결과/예약번호; 부재는 완료가 아님 |
| KTX 취소표 감시·예매 | Pickle 공식 판매자 설명 [P5] | 노선·날짜·시간 범위·예산→감시→허용한 예매. 감시 등록과 실제 승차권 확보 구별 |
| 상품 주문·배송 조회·취소 | Pickle 공식 판매자 설명 [P5] | 정확한 옵션·총액→주문 상태→배송 최신 조회/취소. 주문·운송장·조회시각/취소 영수증 |
| 정부 사이트 서류 신청 | Pickle 작업 문서 [P1] | 문서 종류·제출 목적→인증 인계→작성·제출→접수 확인. 모든 사이트 지원으로 일반화하지 않음 |
| 인증 중 브라우저를 넘겨받았다가 다시 맡기기 | Pickle 브라우저 인계 사용 문서 [P4] | 같은 사용자 세션·작업 지점 유지→인증 완료→재개. 다른 기기 로그인과 혼동하지 않음 |
| 여행 안내서·보험 약관 비교표 만들기 | Manus 공식 시연 모음 [N1] | 문서/웹 근거→조건별 비교→읽기 쉬운 산출물. 안내서는 구매 증거가 아니며 약관 누락은 추정하지 않음 |
| 배송비 포함 특정 가격 아래면 알려주기 | **이번 자료에서 특정 가격 감시 사례는 확인하지 못한 무수리 후보** | 상품·옵션·통화·전체 비용·주기·종료일→동등한 관측 비교→알림. 쿠폰가·옵션 변경 구별 |

Muse [M1]의 사용자 경험은 제작자 자신의 서술이다. Pickle App Store에는 전화 예약
변경 성공 후기와 낯선 발신번호 때문에 상대가 받지 않는다는 후기도 함께 있다 [P5].
이는 시험할 현실적인 실패 조건을 알려주는 미검증 일화이며 제품 간 성능 비교는 아니다.
서비스 연결 목록만으로 그 서비스의 모든 작업이 성공한다고 판단하지 않는다.

## 가져올 구조

Muse 공개 기술 글은 connector별 API와 사용 절차인 SKILL, 격리된 실행 환경,
credential 대리 사용을 설명한다 [M5]. 정확한 메모리 알고리즘이나 도구 생성 성공률은
공개된 근거가 아니다. 무수리는 이미 정한 Rust·SQLite와 사용자별 문맥 격리를 유지한다.

Muse connector 지침은 검색·조건 확인·실행·확인·후속 변경까지 이어지는 기능을 요구한다.
예약 예시는 가상의 서비스이며, 대기나 장바구니를 확정으로 부르지 않고 결과가 불명확할
때 원래 거래 상태를 조회하라고 한다 [M6]. 이 완료 기준을 무수리의 도구 계약에 반영한다.

Pickle의 결제 안내는 이미 특정한 조건으로 진행하라는 요청을 반영하되 조건이 바뀌면
변경분을 확인하고, 연결이 끊겼으면 주문 기록부터 살피는 흐름이다 [P2]. 무수리도
확정된 사용자 요청·기존 credential binding을 재사용한다. 일상적인 조회·분석을 새 승인
화면으로 막지 않고, 실제로 빠진 대상·금액·시간이나 달라진 범위만 질문한다.
단, 무수리 사용자의 최신 지시는 **메일 발송 전에는 매번 최종 내용과 수신인을 보여주고
허락받기**다. Pickle의 동의 정책을 그대로 복사해 이 요구를 완화하지 않는다.

Agent Skills는 작은 이름·설명을 먼저 주고 필요한 본문만 읽는 구조다 [S1][S2]. 따라서
문장마다 새 분류기나 스킬을 만들지 않는다. 모델이 사용 가능한 업무 설명에서 절차를
고르고, 그 절차가 공통 도구를 사용한다. 스킬을 읽었다는 사실은 작업 완료나 권한 부여가 아니다.

## 무수리 구현 순서

1. **현재 저장 자료로 끝까지 할 수 있는 일:** 자료 브리핑·회의 준비·근거 있는 비교와
   정확한 소수 계산. 일반 채팅에서 절차를 읽고 기존 검색/조회로 실제 결과를 만든다.
2. **메일·일정:** 계정별 연결과 검색/thread/초안/라벨, calendar availability/event를
   구현한다. 메일 변경·발송과 일정 생성은 조회와 다른 typed action·영수증을 사용한다.
3. **계속 맡기는 일:** durable 작업/예약/조건 감시/알림. pause/edit/restart, 시간대,
   늦은 답장과 중복 회차를 검증하고 5초 polling을 모든 작업의 실행 주기로 오해하지 않는다.
4. **브라우저·배송·예약:** API가 있으면 native connector, 필요하면 사용자별 브라우저와
   인증 인계. 원래 작업을 보존하고 결과 불명 시 조회부터 한다. 기존 세션·볼트 binding 재사용.
5. **전화·거래·커뮤니티:** 실제 공급자 검증을 거친 전화/주문, 스킬 설치·업데이트·제안 카드.
   성공 이력만으로 임의 코드를 자동 실행하지 않고 재사용 절차를 검증해 개인 스킬 후보로 만든다.

각 단계는 성공 문장 대신 사용자 결과로 평가한다. 예를 들어 배송은 출처·조회시각이 있는
현재 상태, 메일은 draft와 sent 구분, 예약은 확인번호, 감시는 실제 회차 기록이 필요하다.
현재 가능한 기능과 아직 필요한 연결은 [업무 스킬](../task-skills.md)에 구분한다.

## 자료와 확인 범위

아래 링크는 2026-11-13 열람했다. 게시일 없는 도움말은 열람일만 기록한다. 외부 코드,
private skill 원문, 제품 자산은 복사하지 않았다. Pickle 도움말은 SPA라 검색 도구에서
본문이 비어, 공식 main→help 모듈이 불러오는 한국어 문서 번들을 확인했다. 당시 번들
SHA-256은 `4b99b8bbe68445c9c856cf03157b6324aa3e3aa2785656112b0bb10db0837f9a`다.

- [M1: How We Designed Muse](https://muse.ai/blog/how-we-designed-muse), 2026-09-08. 제품 설계·제작자 경험.
- [M2: Introducing Muse](https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/), 2026-09-08 / 페이지 갱신 표시 2026-11-10. 공식 출시 설명.
- [M3: Muse in Singapore](https://about.fb.com/news/2026/10/muse-is-now-available-in-singapore-the-worlds-first-personal-ai-agent/amp/), 2026-10. 공식 지역별 사례; 본문 접근 실패 부분은 검색 색인의 요약만 확인.
- [M4: Meta AI Doesn't Just Think, It Acts](https://about.fb.com/news/2026/07/meta-ai-muse-spark-doesnt-just-think-it-acts/), 2026-07-24. 소비자 Muse와 구별되는 Meta AI/Muse Spark 사례.
- [M5: How We Built Safety Into Muse](https://muse.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse), 2026-09-08. 공개 구조.
- [M6: Muse connector guidelines](https://platform.muse.ai/docs). 공식 규약과 가상의 예약 예제.
- [P1: Pickle 작업 맡기기](https://pickle.com/help/tasks/delegate).
- [P2: Pickle 결제 전 확인](https://pickle.com/help/tasks/confirm-before-payment).
- [P3: Pickle 예약 작업](https://pickle.com/help/tasks/scheduled).
- [P4: Pickle 브라우저 인계](https://pickle.com/help/tasks/take-over-browser).
- [P5: Pickle Company 공식 앱 설명과 공개 후기](https://apps.apple.com/kr/app/id6795790129).
- [Pickle 공식 한국어 도움말 배포본](https://pickle.com/assets/sources-Do5w8fEm.js). 자산 URL은 향후 변경될 수 있다.
- [L1: Lindy email triage](https://docs.lindy.ai/features/inbox-management/email-triage).
- [L2: Lindy follow-up bumps](https://docs.lindy.ai/features/inbox-management/follow-up-bumps.md).
- [L3: Lindy scheduling](https://docs.lindy.ai/features/meeting-assistant/scheduling.md).
- [N1: Manus official use cases](https://manus.im/usecase-official-collection). 시연 모음.
- [S1: Agent Skills specification](https://agentskills.io/specification).
- [S2: Equipping agents with Agent Skills](https://www.anthropic.com/research/equipping-agents-for-the-real-world-with-agent-skills), 2025-10-16.

[M1]: https://muse.ai/blog/how-we-designed-muse
[M2]: https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/
[M3]: https://about.fb.com/news/2026/10/muse-is-now-available-in-singapore-the-worlds-first-personal-ai-agent/amp/
[M4]: https://about.fb.com/news/2026/07/meta-ai-muse-spark-doesnt-just-think-it-acts/
[M5]: https://muse.ai/blog/security-and-safety-for-ai-agents-our-approach-with-muse
[M6]: https://platform.muse.ai/docs
[P1]: https://pickle.com/help/tasks/delegate
[P2]: https://pickle.com/help/tasks/confirm-before-payment
[P3]: https://pickle.com/help/tasks/scheduled
[P4]: https://pickle.com/help/tasks/take-over-browser
[P5]: https://apps.apple.com/kr/app/id6795790129
[L1]: https://docs.lindy.ai/features/inbox-management/email-triage
[L2]: https://docs.lindy.ai/features/inbox-management/follow-up-bumps.md
[L3]: https://docs.lindy.ai/features/meeting-assistant/scheduling.md
[N1]: https://manus.im/usecase-official-collection
[S1]: https://agentskills.io/specification
[S2]: https://www.anthropic.com/research/equipping-agents-for-the-real-world-with-agent-skills
