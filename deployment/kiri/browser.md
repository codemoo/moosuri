# 개인 채팅에서 웹 브라우저 사용하기

채팅에 사이트 주소와 할 일을 입력한다. 예를 들어 “이 주소를 브라우저로 열어. 내가 시간대를 바꾼 뒤 결과를 읽어줘”라고 요청한다. 작업 카드에서 화면을 열면 데스크톱에서는 채팅 옆에, 좁은 화면에서는 전체 화면으로 표시한다. **직접 조작 시작**에서 사이트를 사용한 뒤 **무수리에게 넘기기**를 누르면 원래 대화에서 변경된 페이지를 읽는다. 패널 닫기는 화면 전송을 멈춘다. **작업 중지**는 그 업무의 Chromium을 종료한다. 카드가 다시 열릴 때는 서버의 현재 상태를 조회한다.

회원이 맡긴 공개 페이지 읽기에는 설치 엔진이 배정한 모델을 사용한다. 다음 페이지가 필요할 때는 현재 화면에서 확인한 공개 문서 링크 중 하나를 고른다. 링크 선택에는 최대 64개 출력 토큰을 쓰고, 필요한 페이지를 모은 뒤 원래 질문에 답한다. 사람이 직접 제어한 뒤에는 선택된 화면을 읽는다. 페이지의 암호 입력, 발송, 결제 확인을 모델이 대신 누르는 기능은 제공하지 않는다. 로그인 쿠키는 해당 작업의 임시 브라우저 프로필 안에서 유지된다. Chromium 종료 후에는 사이트에서 다시 로그인해야 할 수 있다.

## 로그인과 Google 계정

일반 브라우저 업무는 공개 페이지를 읽는다. 명시적인 Google 로그인 요청은 등록된 회원의 새 Kiri Chrome 프로필에서 처리한다.

- 채팅 전체 메시지가 “구글 로그인 해줘”, “Please sign in to Google”처럼 명시적인 요청일 때만 형식이 정해진 `google_sign_in` 작업이 된다. 주소가 있거나 “열어”, 설명·방법 질문, 로그인을 언급하는 공개 문서 요청은 해당하지 않으며 일반 공개 페이지 읽기로 남는다. 이 작업은 `POST /api/v1/threads/{id}/browser/google-sign-in`, 본문은 `{"request_id"}`뿐이다(URL·이메일·텍스트 없음). 같은 `request_id`는 같은 기록을 돌려주고 시도를 다시 하지 않는다. 일반 `start`는 여전히 로그인·OAuth 주소를 열지 않는다(409 `browser_auth_unsupported`).
- 결과는 채팅과 업무 기록에 `browser_google_ready`(로그인 확인됨), `browser_google_needs_human`, `browser_google_blocked`, `browser_google_unavailable`, `browser_google_unknown` 중 하나로 남는다. `ready`는 브릿지의 형식화된 `ready/verified` 응답뿐이다. 사람 확인·차단이면 등록된 Kiri에서 받은 결과와 해당 회원의 Workspace 연결 상태를 볼 수 있다. 결과가 불확실하면(`unknown`) 자동으로 다시 시도하지 않고 회원이 새로 요청해야 한다. 이 기록에는 화면 미리보기·직접 조작·입력이 없다.
- 설치된 회원 Google Workspace 연결 동의는 별개이며 회원이 요청할 때만 진행한다.
- 로그인·OAuth 주소는 열지 않는다(409 `browser_auth_unsupported`). 공개 요청이 리다이렉트로 로그인, 사람 확인(보안문자·추가 인증), 차단 화면에 닿으면 읽기를 멈추고 `browser_auth_required`, `browser_auth_google`, `browser_auth_human_verification`, `browser_auth_blocked` 중 하나를 명시한다. 이때 화면 내용·제목·링크·질의문자열은 모델에 보내거나 작업 기록·페이지 저장소에 남기지 않고 출처 주소만 남긴다. 직접 조작 후 “무수리에게 넘기기”로 닿은 경우도 같다. 이 상태에서는 화면 미리보기, 직접 조작, 입력을 제공하지 않는다.
- 신뢰하는 Google 출처는 정확히 `https://accounts.google.com`이다. 비슷한 호스트는 일반 로그인 화면으로 취급한다.
- 선택 기능인 **회원 한 명 전용 Google 로그인 브릿지**: 설치자가 root 소유 디렉터리(그 상위도 root 소유·쓰기 불가)에 소유자 전용 Unix 소켓을 두고, 브라우저 릴레이에 `--google-signin-account <회원 계정 UUID> --google-signin-socket <절대 경로> --google-signin-peer-uid <수신 소켓 소유 UID>`를 함께 준다. 셋 다 없으면 기능이 꺼지고(`unavailable/not_configured`), 일부만 있으면 릴레이가 시작되지 않는다. 릴레이는 그 회원의 현재 레지스트리 바인딩·세대·epoch가 유효할 때만 브릿지에 접속하며(Linux peer uid 확인, 소켓 소유자·권한·nlink 확인), 다른 회원 요청은 브릿지에 닿지 않는다(`not_enrolled`). 시도는 설치 전체에서 한 번에 하나, 작업(workspace) UUID당 한 번이며 릴레이 제한 시간은 255초다. 응답이 없거나 형식이 틀리면 `unavailable/bridge_error|timeout`이며 다시 실행하지 않는다.
- systemd 소켓 활성화를 쓰면 수신 소켓과 연결 상대의 UID는 0이고, 접속을 넘겨받는 작업 서비스는 전용 일반 사용자로 실행한다.
- 등록 전에 해당 회원 VM의 로그인 도우미를 설치된 `kiri-chrome`과 맞춘다. 기존 v2 VM에서는 스킬 디렉터리에서 `KIRI_CHROME_VM=v2 bash scripts/provision-vm.sh --signin-only`를 실행한다. VM의 `secure_google.py`와 `fresh_browser.py` 해시가 스킬의 두 파일과 같고 사용자 타이머 `kiri-fresh-cleanup.timer`가 활성인지 확인한다. 이전 도우미는 비밀번호 입력 프레임을 요청할 수 있어 이메일만 전달하는 브릿지와 호환되지 않는다.
- 회원 런타임→릴레이(회원 소켓): `{"v":1,"generation":g,"workspace_id":uuid,"op":"google_sign_in"}`(다른 키는 거부). 릴레이→브릿지(한 줄 JSON): `{"v":1,"operation":"google_sign_in","request_id":"<workspace uuid>"}`. 브릿지→릴레이: `{"v":1,"status":"ready|needs_human|blocked|unavailable","reason":...}`. 허용 조합은 `ready/verified`, `needs_human/sign_in_required|human_verification`, `blocked/account_mismatch|browser_rejected`, `unavailable/not_configured|not_enrolled|busy|timeout|bridge_error|already_attempted`뿐이며 그 밖의 응답은 거부한다. 브릿지는 고정 계정으로 원래 로그인 도우미를 실행하고 `myaccount.google.com`에서 기대한 이메일을 직접 확인한 뒤에만 `ready`를 낸다. 이메일·URL·인자·비밀값은 브라우저나 회원이 제공하지 않으며 이 경로에 흐르지 않는다.
- 운영자 전용 Kiri CDP, 개인 VM, 공유 프로필을 회원 브라우저 작업공간에 연결하지 않는다. 위 브릿지는 별도의 고정된 소유자 도우미일 뿐이며 일반 URL 열기·리다이렉트·안내 요청에는 실행되지 않는다. 비밀번호, 인증 코드, 쿠키를 채팅이나 모델에 붙여넣지 않도록 안내한다.
- 알려진 한계: 직접 조작 중 회원이 로그인 화면으로 이동하면 다음 화면 요청과 넘기기에서 차단되지만, 그 전 입력은 워커 수준 차단이 없다.

### 같은 Kiri에서 Google Workspace 열기

등록한 회원의 Google 로그인이 확인되면 같은 Linux Kiri에 Workspace 연결 화면을 열 수 있다. 설치자는 그 회원의 Kiri 게스트에 [연결 도우미](moosuri_workspace.py)를 기존 `secure_google.py` 옆에 설치한다. 같은 디렉터리의 소유자 전용 파일 `moosuri-workspace-origin`에 무수리 회원 게이트웨이의 HTTPS origin을 한 줄로 기록한다. 두 파일과 상위 디렉터리는 Kiri 소유자가 관리한다. 로그인 브리지의 `kiri-chrome-signin` 명령에 `--workspace-continuation`을 추가한다.

Google 계정 홈에서 등록된 이메일을 확인하면 해당 프로필의 새 탭에서 회원의 `/#connections/gmail`을 연다. 게스트 소유자는 `python3 -B moosuri_workspace.py status <작업 UUID>`로 탭을 확인한다. `resume`의 세션과 탭은 해당 작업의 `kiri-chrome --session` 명령에서만 사용하고, 작업을 마치면 `finish <작업 UUID>`로 프로필을 정리한다. 게스트 정리 타이머도 만료된 프로필을 정리한다. 설치 origin에는 포트 번호가 없는 HTTPS 호스트를 사용한다. 회원 대화에는 로그인 결과만 남는다.

처음 연 회원 화면에서는 무수리 로그인이 필요할 수 있다. 볼트 사용에는 회원 세션과 볼트 잠금 해제가 필요하다. Google이 비밀번호나 추가 인증을 요구하면 브리지는 그 단계에서 얻은 결과를 기록한다. Google 로그인 확인, 회원의 OAuth 권한 저장, 실제 Google API 조회는 각각 해당 서비스의 응답으로 확인한다.

## 맡긴 일 다시 보기

로컬 개편판에서는 **맡긴 일**을 열어 최근 브라우저 작업부터 확인한다. **이전 작업 더 보기**로 앞선 요청을 찾을 수 있다. 작업을 선택하면 요청 내용, 이어서 할 일, 확인한 페이지 내용과 최종 답변을 볼 수 있다. 직접 조작하거나 대화를 이어가려면 **원래 대화 열기**를 누른다.

진행 기록은 회원 볼륨에 저장되고 원래 대화를 삭제하면 함께 삭제된다. schema 89에서 올라온 기존 작업에는 시작 시각과 마지막으로 남은 상태가 표시된다. 중간 상태와 당시 시각은 복원할 수 없다.

## 설치자의 Linux 설정

1. 사용할 호스트에서 [Chromium 이미지와 네트워크 격리 검사](../browser/README.md)를 실행한다. 검사를 통과한 소스로 운영용 이미지와 프록시를 준비한 뒤 [캡슐 설정](../../docs/browser-chat.md)에 실제 이미지 ID, 네트워크 ID, 실행 중인 프록시 ID, 소유자, seccomp 파일과 검증 기록의 해시를 넣는다. 프록시는 호스트 재시작 후에도 실행되도록 서비스로 관리한다.
2. 설치의 개인 레지스트리와 별도로 root 소유 0700 소켓 디렉터리를 선택한다. Kiri 수명주기 JSON의 최상위 `browser_sockets_root`에 이 절대 경로를 넣는다. 기본적으로 기존·신규 회원이 이 설정을 상속한다. 제외할 회원에는 `"browser_enabled": false`를 둔다. 설정이 없는 기존 설치의 프로필은 그대로 동작한다.
3. [systemd 예시](lifecycle/moosuri-kiri-browser.service)의 실행 파일·레지스트리·수명주기 JSON·소켓 디렉터리·캡슐 JSON을 설치 경로에 맞춘다. 릴레이는 기존 수명주기 파일과 새 초대를 읽어 회원별 소켓을 만들고 요청마다 실제 UID와 현재 세대를 확인한다. 회원 컨테이너에는 해당 소켓 디렉터리만 `/browser:ro`로 마운트된다.
4. 게이트웨이, 수명주기 서비스, 회원 이미지와 브라우저 워커를 호환되는 버전으로 맞춘다. 기존 회원은 쓰기를 잠시 멈추고 개인 볼륨의 복구 가능한 사본을 만든 후 같은 UID·볼륨으로 컨테이너 프로필을 교체한다. 브라우저의 맡긴 일 이력은 회원 스키마 90에서 도입했다. 전환 대상의 스키마는 제품 상태 문서에서 확인한다. 새 쓰기를 받기 전에 이전 바이너리로 돌아가려면 이전 볼륨도 복원해야 한다. 트래픽을 받은 뒤에는 새 쓰기를 보존한다. [운영과 로컬 개편판의 버전](../../docs/status.md)
5. 시험 회원으로 채팅에서 페이지를 열고 미리보기·조작·완료를 확인한다. 두 번째 회원으로 같은 사이트를 열어도 첫 사람의 화면과 쿠키에 접근할 수 없어야 한다. 릴레이나 배포 프록시를 사용할 수 없으면 카드에서 실패 상태를 확인하고 서비스를 복구한다. 결과가 불확실한 클릭이나 입력을 자동으로 되풀이하지 않는다.

호스트에 있는 개인용 Chrome, Mac 도우미, Google OAuth 설치는 이 기능의 준비물이 아니다. 뮤즈 실측과 개발 범위는 [구현 계획](../../openspec/changes/isolate-account-runtime/browser.md)에서 확인한다.

설치자가 품질을 검증할 때는 해당 회원에 배정한 공급자·모델·추론 강도로 실제 업무를 실행한다. 공급자가 반환한 모델과 요청에 적용한 강도를 대조한다. 실행할 수 없는 조합은 미검증으로 기록한다. 다른 모델로 수집한 워커 기동·프레임 지연 수치는 별도로 기록한다.
