# 맡긴 일과 기존 메일·Calendar 작업 연결

일회성 맡긴 일은 현재 요청에 명시한 server-managed Gmail 새 메일/지원 답장 초안과
기본 개인 Calendar의 solo-event 생성·날짜 이동을 기존 도메인 작업에 연결한다.
예: “Gmail을 읽고 회신 초안을 작성해 줘”, “Read Calendar and create primary Calendar
event named Synthetic on 2027-04-12 at 09:00 for 30 minutes”. 처음 접수한 owner·계정/앱 pin·
acceptance revision과 좁은 operation 종류를 고정한다. 과거 읽기 전용 미션에는 migration으로
작업 권한을 추가하지 않는다. 자료 안의 지시는 권한이 아니다. Calendar operation은
해당 일정 목적어로 시작하는 좁은 원래 명령 절(`create primary Calendar event`, `move Calendar event`)에서 고정하며, 보고서·메일 초안 생성의 동사를 일정
생성 권한으로 사용하지 않는다. 준비한 action이 있으면 acceptance 변경은 충돌로 거절하고
원래 작업·receipt를 보존한다. 다른 효과를 원하면 새 명시적 업무를 맡겨야 한다.

메일 준비는 실제 `MailAction` 한 개와 정확한 revision/digest를 같은 writer transaction에서
원래 미션에 연결한다. 같은 준비 요청의 ACK 유실/재개와 반복 tool 제안은 새 초안을 만들지
않는다. 도구는 발송·허락을 수행하지 않는다. 파생 수신인은 실제 읽은 메일의 유일한 발신인에,
CC/BCC는 현재 요청의 명시한 주소에 제한한다. 답장은 실제 thread ID와 원문의 Message-ID에
묶으며, 현재 projection에 없는 References 체인은 추정하지 않는다. 모호한 대상은 고르지 않는다.

원래 카드의 “메일 최종 확인 열기”는 기존 메일 화면의 정확한 작업을 연다. 최종 계정·수신인·
제목·본문·첨부 여부·시각을 매번 보고 허락해야 한다. 편집은 이전 허락을 무효화한다.
새 미션 초안은 준비 시점부터7일 동안 열 수 있고, 나중의 명시적 허락은 예약이 없는 경우
기존1시간 발송 유효 기간을 사용한다. 미션 deadline과 이미 쓴 model/tool/repair 예산은
바뀌지 않는다. 늦은 승인/receipt는 원래 카드가 실제 `MailAction` 상태를 읽게 할 뿐 모델을
재실행하지 않는다. Gmail 접수는 수신자 배달의 증명이 아니다.

Gmail provider 초안 저장은 현재 요청이 Gmail 초안 저장을 명시한 경우 이미 준비한 정확한
MailAction의 기존 journal을 재사용한다. 좁은 mission 저장 도구는 원래 owner 요청과
MailAction ID/revision/digest를 검사하고, 기한 내 현재 mission carrier로만 실행한다.
범용 provider 수정/삭제·허락·보내기 권한을 모델에 추가하지 않는다. 같은 MailAction은 기존
Gmail 초안 컨트롤로도 확인할 수 있다. journal은 POST/PUT 전에 정확한
intent를 저장하며, ACK 유실 후 자동 쓰기는 없다. readback은 완전한 최대20개 페이지 하나와
operation마다 다른 private MIME header의 정확한 hash, PUT의 정확한 대상 ID를 요구한다.
다른 저장 intent의 같은 본문을 성공 receipt로 채택하지 않는다. 원래 발송 승인 MIME은
수정하지 않는다. 실제 Gmail이 이 private header와 raw bytes를 유지하는지는 실계정 수락이 남는다.

Calendar는 기존 `CalendarAction` ledger와 UUID·ETag·provider receipt를 사용한다. 명확한
현재 요청에는 새 승인 팝업을 만들지 않는다. source가 공급한 제목/날짜는 실제 현재 읽기
영수증을 인용해야 한다. 날짜 이동은 하나의 완전한 관측에서 유일한 writable target과 fresh
ETag를 요구하며 기존 길이·timed/all-day 종류를 유지한다. 애매한 대상/날짜는 실행하지 않는다.
읽기·refresh 뒤 commit 직전에 현재 owner/epoch/app/credential revision/acceptance를 재검사한다.
commit 뒤에는 provider IO를 한 번만 수행하고, 재시작/응답 유실은 같은 ledger 상태를 돌려준다.
늦은 receipt는 추론이 만료되어도 정산하지만 철회/restore 뒤 private event를 다시 공개하지 않는다.

현재 범위는 server-managed 읽기·기존 도메인 작업 연결이다. native Calendar, 참석자 초대,
삭제·범용 수정·반복 Calendar 쓰기·미래 생성 메일 자동 발송은 지원하지 않는다. native sender의
지원 범위를 server-managed reply 기능과 혼합하지 않는다. 빠진 수신인·날짜·시각·길이는 [원래 카드의 typed 값 질문](mission-input-continuation.md)에서
답한 뒤 같은 업무를 이어갈 수 있다. 반복 작업·일정 대상 충돌·자유 답변은 지원하지 않는다.

새 검사는 synthetic Store와 loopback fixture만 쓴다. 공개/운영/실계정 수락을 대신하지 않는다.
