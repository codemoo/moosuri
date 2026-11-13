# 개인 브라우저 작업 공간 검증

2026-11-13. Muse의 전용 Linux Chromium/broker/AX 관측과 Pickle의 직접 조작·동일 화면 복귀를
공식 공개 자료에서 확인하고, 무수리의 Rust·React·SQLite 구조로 구현했다.
[조사 근거](../research/browser-workspaces-2026-11-13.md), [사용법과 경계](../browser-workspaces.md),
[현재 상태](../status.md)가 실제 제공 범위를 정한다. 제품 내부 코드나 전체 기능을 복제했다는 뜻은 아니다.

## 구현 범위

사용자·대화별 전용 capsule은 여러 도구 호출과 앱 연결 사이에도 같은 프로필·페이지를 유지한다.
React 카드에서 화면 보기·직접 조작·명시적 완료를 제공한다. 완료는 workspace 상태 전환과
후속 사용자 turn을 한 SQLite transaction으로 저장하며, 중복 완료는 같은 turn을 반환한다.
이전 실행 ID를 부활시키지 않고 같은 대화 문맥과 현재 브라우저를 새 실행에 연결한다.

모델 profile 5는 공개 페이지 열기·현재 페이지 관측·사람에게 인계만 받는다. 직접 조작 중
관측은 차단되고, 조작 이력이 있는 workspace의 모델 navigation은 계속 차단된다.
직접 조작5분 유휴는 사용자 대기로 돌아가며 자동 재개하지 않는다. workspace는30분 유휴/
8시간 상한이며 재시작은 결과 불명으로 표시한다. 다른 사용자, 오래된 controller와 폐기된
세션은 frame/input을 사용할 수 없다. 메일 발송의 최종 내용 확인·명시적 승인은 그대로다.

## 발견한 결함과 수정

- 종료 경로가 browser cleanup 전에 끝날 수 있었다. task6초·mail2초·browser7초 drain을
  outer16초가 포함하도록 바꾸고 실제 종료 자원 검사를 추가했다.
- 완료 상태와 후속 turn이 별도 transaction이었다. 원자적으로 저장하고 실패 rollback과
  동일 request 결과를 검사했다.
- janitor가 map에서 제거한 entry를 기다리던 호출이 다시 쓸 수 있었다. mutex 획득 후
  현재 registry의 Arc identity를 재검사하며 실제 대기 경쟁을 재현했다.
- 오래된 화면 proof가 focus/overlay 변화 일부를 놓쳤다. navigation·geometry·scroll 외에
  interaction style8개·paint order·focused backend ID를 묶었다. 값·caret/color animation은
  proof에 넣지 않으며 한 번 사용한 token은 재사용할 수 없다.
- 실제 Chromium DOMSnapshot은 비텍스트 root의 빈 style row, 선택 문자열의 `-1` sentinel,
  문자열 index인 frame ID를 반환했다. 정상 형식을 수용하되 잘못된 index·필수 identity 누락·
  form/editable subtree 값은 계속 거절하도록 수정했다.
- API 시험의 첫 실패들은 합성 model이 tool result의 `kind/value` wrapper와 텍스트의
  공백을 잘못 해석한 harness 오류였다. 반환 계약에 맞춰 수정하고 실패 기록을 보존했다.
- 로그인 이동 중 frame409를 입력 실패로 처리하던 React 동작을 수정했다. 최신 상태에서
  같은 workspace/revision/조작권임을 확인한 뒤 화면만 다시 읽는다. 입력은 재실행하지 않고,
  다른 controller나 계정/대화 변경 시 token·pixels를 제거한다.

- 실제 API 로그인 직후 capture가 끊기는 현상을 별도 worker에서도 재현했다. 수정 전
  직접 시험2/12, CDP method만 기록한 시험3/12에서 `Page.captureScreenshot` 응답이15초 동안
  없어 workspace 전체가 종료됐다. screenshot 읽기만2초 뒤 폐기해 `StalePage`로 반환하도록
  바꿨다. 늦은 응답은 증가한 command ID로 배제하며, 입력/navigation의 불명·종료 규칙은
  유지한다. 실제 WebSocket 회귀 검사는 재전송0과 다음 응답에 옛 캡처가 섞이지 않음을 확인한다.

Store/runtime/connector의 고정 snapshot을 독립 검토하고 결함 수정 뒤 재검토했다.
마지막 React 세 파일과 screenshot timeout의 Rust 두 파일도 별도로 검토했으며 추가 material finding은 없었다. 검토와 실제
브라우저 시험은 서로 다른 증거이며, 소스 검토만으로 Linux 격리를 통과했다고 하지 않는다.

## 실행 증거

- React91개 행동 테스트와 strict TypeScript/production build를 통과했다. 브라우저 전용12개는
  계정/대화 전환·늦은 frame·입력 결과 불명·완료 중복·다른 탭 controller·이동 중 frame 충돌을 다룬다.
- 실제 Chromium156.0.8078.79, Debian12, Linux5.15.0-179-generic의 첫 최종 smoke는
  31commands(22success/9expected rejection),6workers 중2workspaces,87.689초(build 포함)였다.
  HTTPS POST 로그인/Secure cookie 유지·새 profile 격리·form secret 제외·지연 후 타이핑·
  geometry/overlay/focus/replay 거절·기존 private-resource/egress/rebinding 검사를 통과했다.
  해당 tester의11개 시험/진단 project는 모두 container/network0, cleanup error0이었다.
- 그 smoke의 공개 source349files manifest SHA-256은
  `7a0c92d0901e876591e8f3d330f39900f584e412381bdef0b0fe8caf131115a5`다.
  결과 digest는 `c04eef3cf0009a3952afa2aec1f81f0b158bed364051f7274b007ec21edd3a50`,
  로그 digest는 `00803d662bbbbd78edc25aa48e4420cf042291a68ce0edd6a5509c70d697d467`다.
- screenshot 수정 뒤 같은 Linux 조건에서 전체31commands를 다시 통과했다(82.690초).
  로그인 직후 별도 대기 없이 화면을 요청한12/12 trial이 같은 worker와 로그인 cookie를 유지했다.
  6/12에서 screenshot이 약2초 뒤 `StalePage`가 됐지만, 읽기 갱신으로 복구했고 worker 종료0이었다.
  두 최종 시험 project 모두 container/network0·cleanup error0이며 source350files를 전후 대조했다.
  source manifest: `4394656cc8ab10ab91f033568073af222a8961b8e2e3bd2812a34bef0b545d0c`.
  smoke result: `cad6c3081cd008ffcb0c4a6cfc1a7a171c7924610c012de16378fc9221a2c8a7`.
  repeated-navigation result: `cfdf7fb7c99a20d2a65ee447d879c9fbbbd0372b3a560fe918d3c51acc1c9f8e`.
- 합성 model server → 실제 Rust HTTP API·SQLite → 격리 Linux Chromium의 전체 여정11개
  조건을 통과했다. model의 open/handoff, 사람 조작 중 관측 중지, 연결을 끊었다가 같은
  workspace 조회, HTTPS 로그인 cookie를 보존한 현재 페이지 관측, 완료 중복의 같은 turn,
  인증된 model navigation 차단, 타 사용자 read/control 거절, fresh profile cookie 없음,
  새 controller의 이전 token 차단, 비밀번호 canary의 model 미전달, 종료 cleanup/재시작 무재생을
  확인했다. 실메일 호출0, model secret 탐지false, cleanup error/container/network0이었다.
  결과 digest는 `50fb71feac69e2be5ee95a3ef5f829cfeb8dbbb79b1c629d4a8703a00d2b16e9`다.
- 최종 Linux Rust workspace는291개 통과, 명시적 실모델 opt-in1개 ignored다.
  CLI2·installation1·agent52·connector115·server2·HTTP24·Store95이며 Store 내부의
  별도 프로세스1개 실행을 중복 합산하지 않았다. build10.633초, 전체 test189.079초였다.
  macOS에서는 최종 browser34개 회귀·workspace/all-target clippy(`-D warnings`)·fmt를 통과했다.
  Rust 생성 DTO와 TypeScript 선언도 일치했다.
- 별도 합성 deployment harness4개와 shell 문법 검사를 통과했다. 계획 검증과 OpenSpec strict,
  최종 공개 source 허용 목록·비밀 탐지를 확인했다. 새 dependency와 제3자 asset은 추가하지 않았다.

## 검증의 한계

합성 HTTPS fixture·별도 계정/DB·합성 model protocol을 사용한다. 실메일·사용자 credential이나
유료 모델 호출은 없다. 실제 API 여정은 비서 모델의 자연어 업무 품질 점수가 아니다.
데스크톱1280×720과390×844의 화면 확인은 실제 component를 사용한 합성 UI 검증이며,
실제 iOS/Android 또는 운영 사이트의 end-to-end 성공으로 표현하지 않는다.

장기 profile 복구·vault 자동 로그인·범용 자동 폼/예약/구매는 아직 없다. form 값은 관측에서
제외하지만 사이트가 일반 본문에 반사한 모든 비밀을 제거한다고 주장하지 않는다.
서버 관리 쿠키는 악성 관리자에 안전한 사용자 키 볼트가 아니다. 이 단위는 운영 배포나
기존 비서·수집기·cron·데이터 전환을 수행하지 않는다. CI/Actions는 계속 비활성이다.
