# 홈 화면에 설치하기

운영 서버의 HTTPS 주소에서 moosuri를 열면 지원 브라우저의 설치 기능을 사용할 수 있다.
Chrome에서 설치가 가능하면 사이드 메뉴에 **앱으로 설치**가 나타난다. Safari에서는
브라우저의 **공유 → 홈 화면에 추가**를 사용한다. 설치 앱은 기본 채팅(`/app`)에서 시작한다.

공개 HTML·JavaScript·CSS·아이콘만 오프라인 캐시한다. 개인 API·메시지·이미지·암호와
미전송 API 요청을 서비스 워커에 저장하지 않는다. 아래의 5분 one-use 공유 handoff만 별도
IndexedDB에 제한된 평문으로 보관한다. 공개 Cache API에는 넣지 않는다. 로그인과 개인 자료 조회에는 서버 연결이
필요하다. 이미 열린 화면의 초안은 연결이 끊겨도 그대로 남으며 자동으로 전송하지 않는다.
같은 서버 로그인 세션을 확인한 뒤에는 현재 탭의 `sessionStorage`에서 제한된 미전송 초안과
결과 불명 요청 identity를 복구한다. 먼저 현재 대화 목록/기록을 읽고 자동 전송하지 않는다.
서비스 워커·오프라인 캐시에 저장하는 기능은 아니다. 최대16slot/128KiB/8시간과 텍스트8192
UTF-8 byte 범위를 사용하며, 로그아웃·인증 전환·암호 변경은 같은 탭의 소유 key를 즉시 지운다.

이 탭 저장소는 평문이며 볼트가 아니다. 브라우저의 탭 복제·crash/session 복원 동작과 같은
origin의 script/기기 접근을 신뢰한다. 탭 종료 뒤의 완전 삭제나 crash 뒤 복구를 보장하지 않는다.
다른 로그인·제한 세션은 이전 초안을 복구하지 않으며, 자세한 경계는 [대화](chat.md)를 따른다.

처음 오프라인으로 앱을 열었다면 연결이 돌아올 때 다시 로그인 상태를 확인한다.
새 버전은 열려 있는 탭을 강제로 새로고침하지 않는다. 기존 앱 창/탭을 모두 닫고 다시
열면 준비된 새 버전이 적용된다. 개발 Vite 화면에는 서비스 워커를 등록하지 않는다.

설치·공개 shell과 별도로 [기기 알림](web-push.md)은 사용자가 켜는 generic Web Push를 제공한다.
개인 본문은 잠금 화면에 넣지 않으며 계정·기기 binding과 현재 인증을 확인한다.
텍스트 공유받기의 구현 범위는 아래와 같다. 실제 iOS/Android 공유 대상 등록·권한·background
표시 정책은 별도 실기기 검증이며 해당 기기 시험 완료를 주장하지 않는다.
아이콘 제작 근거는 [자산 기록](design-assets.md)에 있다.

녹음 원본과 아직 넣지 않은 음성 인식 글은 공개 shell/서비스 워커/브라우저 저장소에 넣지 않는다. 화면의 녹음은 새로고침 후 복구되지 않는다. 음성을 사용자가 명시적으로 입력창에 넣은 뒤에는 일반 채팅 초안의 정확한 계정/세션 복구 경계를 따른다.

## 다른 앱에서 글 공유받기

지원하는 설치 브라우저에서 제목·글·URL을 공유할 수 있다. 파일·사진·음성은 받지 않는다.
공유는 같은 origin의 정확한 navigation `POST /share-target` multipart만 받는다. 본문은
읽는 중 32KiB/8초로 제한하고 브라우저 multipart parser에 넘긴다. 중복·알 수 없는 field,
파일, 빈 글과 합계8192 UTF-8 byte 초과는 거절한다. HTML·URL도 편집할 평문이며 URL을
가져오거나 실행하지 않는다. 본문은 URL/query·로그·공개 cache에 넣지 않는다.

별도 IndexedDB는 브라우저 기기 ID·generation·의도한 계정·원래 session namespace와
최대 한 payload를 5분 absolute TTL로 보관한다. 아직 claim하지 않은 글은 worker 종료 뒤에도
복구할 수 있다. 최신 공유는 이전 대기/claim을 교체하며 이전 창의 insertion은 실패한다.
32개 이하의 payload 없는 재사용 방지 tombstone을 원래 TTL까지 유지한다. 만료는 접근 때
검사하고 payload를 지우며 열린 review도 만료 때 숨긴다. 닫힌 브라우저의 디스크에서 정확히
5분에 삭제하는 보장은 아니다. 브라우저 profile·같은 origin script를 신뢰하는 임시 평문이며
볼트 암호화·private offline queue·API 재전송 기능이 아니다.

항상 서버의 현재 session을 새 GET으로 확인한다. 원래 계정의 일반 session 만료 뒤에는
5분 동안 글을 숨겨 보관할 수 있다. 같은 계정으로 로그인한 뒤 **이 계정의 초안으로 가져오기**를
직접 눌러야 한다. 다른 계정으로 로그인하면 이전 글을 지운다. 자발적 로그아웃·암호 변경과
관측한 cross-tab 인증 철회는 즉시 화면 lifetime을 닫고 durable generation을 회전해 글을 지운다.
아직 인증한 적 없는 기기에서401을 받은 공유는 버리고 **로그인한 뒤 다시 공유해 주세요**를
보인다. 네트워크 실패는 인증 증거가 아니며 글을 표시하지 않는다. 401에서는 글을 숨겨 두지만 재로그인만으로 복구를 허락하지 않는다. 원래의 비인증용 session
namespace와 지금 claim한 세션의 비인증용 namespace를 보존하고 공개 URL이 아닌
`POST /api/v1/shares/origin` 본문의 `origin_scope`·`expected_current_scope`로 검증한다.
서버는 실제 cookie 세션이 expected-current namespace와 정확히 일치해야 허락한다. 마지막
session GET 뒤 같은 계정의 새 로그인으로 cookie가 바뀌어도 이미 claim한 글을 옮기지 않는다.
아직 claim하지 않은 글을 같은 계정의 새 세션에서 직접 가져올 때는 원래 origin과 새 current
namespace를 각각 확인한다. 서버는
현재 unrestricted UserContext의 정확한 tenant/account만 최대256개 session row를 읽고,
원래 row가 현재 auth epoch의 unrestricted row로 남아 있을 때만 허락한다. 자연 만료 row는
남지만 개별 logout·관리자 세션 철회·암호 변경/재발급·정지/복구의 삭제/epoch 변경은 거절한다.
관리자도 다른 계정의 origin을 확인할 수 없다. origin은 bearer가 아니며 서버 자료 접근권이 없다.
검증 실패는 공유를 지우고 다시 공유하게 한다. 256개를 넘으면 허락을 추측하지 않고 닫는다.

claim은 단일 IDB transaction으로 한 창에만 준다. claim/consume 완료 callback이 다른 탭의
철회 뒤 늦게 전달되어도, 새 session과 origin 서버 검증 뒤 최종 durable generation/claim을
다시 확인한다. 화면 공개/초안 insertion은 그 IDB transaction 안의 동기 callback에서만 한다.
다른 탭 철회 broadcast는 즉시 이미 표시한 글을 숨기지만 broadcast를 lock으로 믿지 않는다. claim 뒤 새로고침·뒤로가기·다른 창에
자동으로 양도하지 않는다. 공유 글을 편집한 다음 **초안에 넣기**를 누르면 처음 선택했던
대화의 최신 초안 끝에 붙인다. 대화를 바꾸면 원래 대화로 돌아와야 한다. 합계8192 byte를
넘으면 글을 줄여야 한다. 먼저 consume transaction으로 payload를 지운 뒤 초안에 넣으므로
그 사이 창 종료/초안 변경으로 실패하면 글을 잃을 수 있다. 안전한 재시도는 다시 공유하는
것이며 자동 복구로 중복 삽입하지 않는다. 평소 **보내기**만 chat/LLM/tool 요청을 만든다.

저장소 차단·quota·손상·만료·이미 claim한 URL은 다시 공유하라는 일반 안내로 끝낸다.
공유 편집창은 채팅의 같은 스타일을 사용하며 삽입 후 작은 안내만 남기고 현재 입력창으로 돌아온다.
독립 검토·Native/Linux·실제 Chrome의 multipart navigation과 서비스 워커 재시작 검사를 통과했다.
Chrome 설치 가능성 검사도 오류가 없었지만 실제 OS 공유 메뉴·iOS/Android 기기 검증을 뜻하지 않는다.
[검증 기록](reviews/2026-11-14-incoming-pwa-share.md)과 F16의 남은 실기기 gate를 따른다.
