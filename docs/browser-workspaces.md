# 개인 작업 브라우저

뮤즈를 주 기준으로 브라우저를 작업 중 유지하고, 피클의 직접 조작·동일 화면 복귀 흐름을
채팅에 연결한다. Chromium 엔진을 바꾸는 대신 Rust broker가 브라우저 수명과 조작권을
소유한다. [조사 근거](research/browser-workspaces-2026-11-13.md), [현재 검증](status.md)을 함께 읽는다.

## 사용 흐름

대화에서 웹 업무를 맡기거나 브라우저 카드에 공개 URL을 입력한다. 에이전트의
`browser_open`은 대화에 속한 격리 브라우저를 열고 같은 프로필·페이지를 유지한다.
`browser_observe`는 URL을 다시 열지 않고 현재 화면을 읽는다. 간단한 공개 조사에는
기존 `web_read`·`web_search`·일회성 `browser_read`도 계속 사용한다.

카드에서 화면을 보다가 **직접 조작**을 누르면 사람이 조작권을 갖는다. 클릭·스크롤·
텍스트 입력·키보드·주소 이동을 제공하며 로그인/MFA도 사용자가 이 화면에서 처리한다.
**완료하고 이어서 처리**는 같은 대화에 보이는 후속 지시를 한 번 저장한다. 에이전트는
같은 브라우저의 현재 문서를 다시 관측한다. 원래 실행 ID를 되살렸다는 뜻은 아니며,
이전 지시·대화 문맥을 가진 새 turn이다. 응답 유실 후 완료 재확인은 같은 request ID를 쓴다.

직접 조작 중 모델의 관측과 이동은 멈춘다. 다른 탭이 조작권을 가져가면 이전 token과
revision은 즉시 무효다. 조작권은 활동 없이 5분이 지나면 **사용자 확인 대기**로 돌아가며
에이전트를 자동 재개하지 않는다. 앱을 닫거나 채팅 스트림이 끊겨도 서버 브라우저는 유지한다.

브라우저는 입력/관측 없이 30분 또는 시작 후 8시간에 만료된다. 사용자당 최대 2개,
설치의 capsule 동시 실행 한도도 적용한다. 종료·만료 시 세션의 프로필과 쿠키를 폐기한다.
서버 재시작은 **결과 확인 필요**로 표시하며 과거 클릭/로그인/발송을 재생하지 않는다.
장기 디스크 프로필과 장애 후 동일 탭 재접속은 아직 구현하지 않았다.

## 모델과 사람의 경계

모델에는 URL 열기·현재 페이지 관측·사람에게 인계하는 세 도구만 제공한다. CDP endpoint,
임의 JavaScript·마우스·키보드·스크린샷·password/TOTP 입력 도구는 노출하지 않는다.
사용자가 한 번이라도 조작권을 얻은 workspace에서는 모델의 URL 이동도 계속 차단한다.
로그인된 페이지는 관측하고, 추가 이동/폼 작업은 직접 조작 카드로 맡긴다.

메일은 기존 Gmail의 동결된 최종 수신인·본문·첨부 확인과 **허락하고 보내기**를 따른다.
범용 브라우저 자동 클릭으로 이 단계를 우회하지 않는다. 사람이 직접 화면에서 누른
전송은 그 사람의 직접 조작이며 에이전트 자동 발송으로 보고하지 않는다.
자동 로그인 adapter·범용 자동 폼 작성/예약/구매는 현재 구현 범위 밖이다.

관측은 CDP Accessibility와 DOMSnapshot의 레이아웃 텍스트를 사용한다. AX에서 빠진
눈에 보이는 `aria-hidden` 부정 표현도 보존하며, editable/form subtree의 값은 제외한다.
페이지가 입력값을 일반 본문에 되비추는 모든 상황까지 제거한다고 주장하지 않는다.
사용자는 직접 조작을 마치기 전에 모델에 보여줄 현재 화면을 정한다. 프레임·입력값·
control token은 일반 채팅/도구 결과/SQLite 기록에 저장하지 않는다.

브라우저는 서버가 실행하므로 로그인 쿠키와 화면에 서버 운영자가 접근할 가능성이 있다.
이 기능을 사용자 키로만 해독하는 vault 또는 악성 관리자 방어라고 표현하지 않는다.
vault credential 주입은 [별도 CredentialBroker 계약](credential-use.md)에 남아 있다.

## 구현·운영

- SQLite migration 8에 사용자/대화/workspace ID·상태·revision·조작권 hash를 둔다.
  API의 계정 ID로 권한을 만들지 않고 인증한 UserContext와 현재 session/epoch를 확인한다.
- Rust broker는 대화별 controller를 직렬화한다. SQLite writer를 잡은 채 Chromium IO를
  기다리지 않는다. 입력을 전송한 뒤 응답이 불명확하면 자동 재입력하지 않는다.
- 사람에게만 전달하는 frame token은 한 번 사용하며 document·navigation·레이아웃·
  짧은 수명에 묶는다. 화면의 애니메이션이나 입력 caret만으로 조작을 막지 않는다.
  로그인 이동 중 screenshot 응답이 멎으면 캡처만 2초 뒤 폐기하고 같은 브라우저를 유지한다.
  React는 최신 조작권을 확인한 뒤 화면만 다시 읽으며, 클릭/입력은 자동 재실행하지 않는다.
- profile 5만 새 도구를 받는다. 이미 저장된 profile 1–4 실행에 workspace 권한을
  추가하지 않는다. 성공한 typed slot만 `browser_page` 근거를 만든다.
- 기존 `--browser-capsule-file`로 활성화한다. [Linux capsule 구성](browser-chat.md)의
  고정 image/network/proxy·public egress·Chromium sandbox·자원 제한을 그대로 적용한다.
  이미지에 새 `--workspace` worker가 포함되어야 하며 새 이미지로 smoke를 다시 확인한다.

주요 API는 `/api/v1/threads/{id}/browser`와 `/frame`, `/takeover`, `/input`, `/navigate`,
`/resume`, `/close`다. 원격 CDP 포트를 웹에 공개하지 않는다. React는 인증된 요청으로만
프레임을 받아 메모리에 표시하고 계정/대화 전환 시 이미지·입력·token을 제거한다.
