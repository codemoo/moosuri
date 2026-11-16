# 설치·관리자·사용자 흐름

[한 줄 설치와 한국어·영어 CLI 안내](install.md)를 먼저 읽는다. Rust `setup`은 접속 방식,
관리자, 선택적 API/local 모델 연결, 사용자 서비스와 대시보드 진입을 묶는다. 아래에는
이 흐름과 함께 기존 웹/운영 설정의 전체 계약을 보존한다. API 키는 숨김 입력이며
임시 암호는 첫 로그인에서 변경해야 한다. ChatGPT 구독은 인증한 웹 모델 설정,
Claude 구독·Google·초대 발신자·암호화 백업은 문서에 명시한 별도 설정을 따른다.
패키지 공개와 실제 플랫폼 수락 범위는 [현재 상태](status.md)·[검증 기록](reviews/2026-11-17-guided-installation.md)을 따른다.

## 설치 방식

macOS Apple Silicon/Intel 및 Linux x86_64/arm64(WSL2 포함)의 단일 Rust binary + 내장 React 정적 파일을 제공하는 구조다. SQLite는 함께 동작하므로 PostgreSQL·Redis를 설치하지 않는다. 서명/checksum을 확인한 release를 받아 `moosuri init` 후 `moosuri serve`로 시작한다. OCI image와 최소 Compose 예제는 같은 설정·private volume을 사용한다. container 없이도 동작해야 한다.

기본 `data_dir`는 OS private data 위치이며 소스·Dropbox·공유 네트워크 파일시스템 바깥이다. owner-only directory/file, WAL/SHM·journal·backup 경로를 함께 준비한다. 설치기는 binary checksum·쓰기 권한·여유 공간·SQLite version·origin·port·시간대만 필요한 만큼 검사한다. 외부 모델/Google 미연결 상태도 오류 없이 관리 화면에 진입할 수 있다.

## 초기 마법사

1. **접속 방식:** 로컬 컴퓨터의 loopback HTTP가 기본이며, Tailscale 또는 일반 HTTPS self-host도 선택한다. 외부 주소는 설치자가 준비한다.
2. **최초 관리자:** 이름·이메일·비밀번호를 입력한다. 비밀번호 생략 시 충분한 난수의 임시 비밀번호를1회 보여준다. DB에는 Argon2id 해시만 저장하고 `must_change_password=true`다. 설치 중 비밀번호 입력은 terminal echo/log·URL·shell history에 남기지 않는다. 임시 비밀번호의1회 표시는 인증된 setup UI 또는 controlling TTY로만 전달하며 stdout/stderr·로그 수집 출력에 섞지 않는다. TTY 없는 설치는 권한을 제한한 입력 파일/FD로 비밀번호를 제공하고 자동 생성 비밀을 로그에 출력하지 않는다.
3. **LLM 연결:** OpenAI API, Anthropic API, OpenRouter, local/compatible endpoint, 지원되는 ChatGPT plan 연결, Claude 공식 개인 실행 연결을 고른다. [providers](providers.md)의 지원 제한을 그대로 표시한다. 나중에 설정할 수도 있다.
4. **기본 모델·effort:** 실제 연결에서 읽은 model catalog와 probe 결과로 선택한다. 이전 사용자의 private 이전은 기존 Terra/high 선택을 보존하고, 새 설치에서 없는 모델을 강제로 지정하지 않는다. effort 미지원은 숨기거나 설명한다.
5. **초대 메일:** 운영자가 별도 gws 시스템 발신자를 명시적으로 설정한다. 현재 구현은 아래 CLI 설정이며 통합 웹 OAuth 마법사는 미구현이다. 설정한 발신 주소·credential의 유효성은 운영자가 관리하고 설정됨을 실제 Google 계정/scope 확인 완료로 표시하지 않는다. 건너뛰어도 로컬 초대 정보 전달로 계정을 만들 수 있다.
6. **백업 설정(선택):** public recipient를 등록하거나 백업 복구 묶음을1회 내보내 서버 밖에 보관한다. 별도 복사본으로 작은 합성 암호문을 여는 검사를 거쳐 자동 backup을 켠다. 건너뛰면 unconfigured로 표시하며 일반 앱 사용은 가능하다. 사용자 vault master를 요구하거나 이 묶음에 넣지 않는다. 키 소유·교체·복원 입력은 [백업 설계](architecture.md)의 계약을 따른다.
7. **완료:** 사용자 앱 `/app`과 관리자 `/admin` 주소, backup 상태·계정/연결 상태를 보여준다. admin 외 계정은 자동 생성하지 않는다.

bootstrap endpoint는 빈 설치에서만 열고 일회 비밀·loopback 또는 설치 세션으로 보호한다. 동시에 두 init 요청이 와도 관리자 한 명만 생성한다. 설치 완료 후 endpoint는 닫힌다. 최초 admin이 입력한 정식 비밀번호는 force-change 대상이 아니며 생성된 임시 비밀번호만 강제 변경한다.

## 임시 비밀번호와 초대

관리자는 이름·이메일·role(default member)을 입력하고 계정을 만든다. 기본 임시 비밀번호 수명은24시간, 1회 사용 후 변경을 요구한다. 재발급은 이전 비밀번호·초대 세션을 폐기한다. 존재 여부를 노출하지 않는 오류와 이메일+IP rate limit, bounded 해시 비용을 적용한다.

첫 로그인은 **비밀번호 변경 전용 제한 세션**만 발급한다. 이 세션은 채팅·LLM·자료·vault·관리자 API에 접근할 수 없다. 새 비밀번호 저장, 기존 임시 credential 및 제한 세션 폐기, 새 정식 세션 발급을 한 transaction으로 처리한다. URL에는 비밀번호를 넣지 않는다. 초대 메일은 로그인 URL·이름·만료 안내를 담고, 임시 비밀번호 전달을 선택하면 한 번만 발송한 뒤 서버 평문 보관을 제거한다. 관리자 화면에는 일회 복사 방식도 제공한다.

gws 연결이 없거나 발송이 실패해도 계정 생성 자체를 잃지 않는다. `created / delivery_pending / sent / delivery_unknown / expired / activated`를 구분한다. 응답 유실 시 동일 invitation/effect ID로 조회한다. send-only scope로 조회할 수 없으면 delivery_unknown을 유지하고 자동 재발송하지 않는다. 자동 resend가 임시 비밀번호를 새로 만들거나 이메일을 중복 발송하지 않게 한다. 재초대는 이전 비밀번호를 폐기하는 명시적 관리 동작이다.

Google 소셜 로그인은 초기 필수 기능으로 가정하지 않는다. Google 연결의 첫 목적은 초대 발신과 각 사용자의 Gmail/Calendar connector다. 향후 소셜 로그인을 추가하더라도 기존 local user와 검증된 동일 이메일 연결을 명시적으로 처리한다.

## 관리자 대시보드

| 화면 | 가능한 일 | 표시하지 않을 정보 |
| --- | --- | --- |
| 사용자 | 생성·초대·재발급·정지·삭제·role, 최근 활성·할당량 | 비밀번호, 다른 사용자의 private 대화/기억/볼트 본문 |
| 모델 연결 | 공유 API/local 설정, 구독 지원 상태, secret 교체·test·활성화 | 저장한 key/token 재노출, 개인 subscription token |
| 기본 모델 | model/effort·허용 모델·사용자 override 정책 | 지원하지 않는 모델을 가능한 것처럼 표시 |
| 사용량 | 사용자/기간/provider/model의 호출·token·지연·오류·예약소진 | 프롬프트/답변/파일·메일 본문 |
| 메일 | 시스템 sender·scope·연결 재인증·초대 delivery 상태 | 일반 사용자의 Gmail token/내용 |
| 운영 | 백업/복원 readiness·버전·저장용량·queue·네트워크 mode | raw trace·암호·원문·DB dump |

단순한 사용자 앱에는 관리자 메뉴를 표시하지 않는다. 관리자도 자신의 대화·메모리·Google·볼트는 하나의 일반 사용자 환경으로 사용한다. role이 있다고 repository의 tenant 제한을 자동 해제하지 않는다.

## 네트워크 모드

**Tailscale:** 같은 self-host 서버를 tailnet 안에 공개한다. 앱은 loopback/Unix socket에 bind하고 Tailscale Serve로 HTTPS를 제공한다. 초대받은 사람에게 tailnet 접근과 moosuri 계정이 둘 다 필요하다. Tailscale identity header는 신뢰한 proxy 경로에서만 읽으며 앱 계정 로그인을 생략하지 않는다. Funnel은 인터넷 공개 기능이라 기본 활성화하지 않는다. 서로 다른 vault origin이 필요한 경우 별도 서비스 이름/host 구성이 가능한지 doctor가 확인한다.

**일반 self-host:** 사용자 도메인·TLS reverse proxy로 `/app`·`/admin`을 제공한다. vault는 별도 origin이다. exact Origin, secure cookie, SSE buffering off, upload cap, websocket/stream timeout 설정을 제공한다. trusted proxy 설정 외 헤더로 사용자·role을 정하지 않는다.

둘 다 self-host 방식이다. Tailscale은 네트워크 노출 선택지이며 저장소·사용자 모델이 다르지 않다. mode 변경은 origin/초대 URL/OAuth callback/Push subscription 재등록/vault logout 이동을 점검하고 session revocation 정책을 적용한다. 주소 변경 후 과거 deep link와 session cookie를 무조건 재사용하지 않는다.

## 로컬 LLM 연결 UX

Ollama·LM Studio·vLLM preset과 generic base URL을 제공한다. server가 실행되는 기기의 `localhost`는 사용자의 브라우저 PC가 아님을 설명한다. container 사용자는 host gateway 또는 LAN/Tailscale URL을 지정한다. 로컬 endpoint가 꺼졌을 때 클라우드로 몰래 우회하지 않는다. endpoint reachability→model list→text/stream/tool probe 순서로 확인하며 유료 probe는 짧게 명시한다.

## 개인 Google Calendar 연결

설치 Google OAuth 웹 앱과 callback은 한 번 준비한다. Calendar를 쓰면 같은 프로젝트에서 Google Calendar API도 사용 설정한다. 각 사용자는 [내 Google Calendar](calendar.md)에서 별도 scope로 동의한다. 기존 Gmail 연결이 Calendar 허용을 뜻하지 않으며 Gmail token을 재사용하지 않는다. 모델 연결 없이도 직접 양식으로 사용한다.


## 선택 시스템 초대 발신자

운영자는 별도 native **gws0.22.5**와 최소 `gmail.send` scope의 전용 credential을 준비한다.
이 저장소나 현재 Mac의 기존 로그인은 자동 사용하지 않는다. npm의 JavaScript wrapper
대신 내려받은 실제 native gws binary의 절대 경로를 지정한다. credential JSON은0600,
전용 gws 설정·임시 발송 디렉터리는0700이며 symlink final path는 받지 않는다.
사용자 개인 Gmail/모델 연결과 별개다.

```sh
moosuri --data-dir /private/moosuri serve --origin https://YOUR-HOST \
  --invitation-gws /private/bin/gws \
  --invitation-gws-credentials /private/invitation/credentials.json \
  --invitation-gws-config /private/invitation/gws \
  --invitation-from system@example.com
```

설정이 없거나 발송 실패해도 계정 생성·일회 암호 복사는 유지된다. 이 옵션은 운영자가
지정한 발신자라는 뜻이며 Google profile/scope의 실계정 검증 완료 표시가 아니다.
gws/Google OAuth의 설치·로그인·credential 회전은 운영자가 별도로 수행한다.
다른 gws 버전은 send 전에 거절하며 해당 버전의 native 입력/재시도 동작을 별도 검토해야 한다.

**People**에서 계정을 만들거나 명시적으로 암호를 재발급하면 기존 일회 암호 창에
**초대 이메일**이 나타난다. 암호 포함은 선택 사항이다. **전체 내용 준비**는 이름·계정
이메일·로그인 URL·credential 만료·전체 본문과 From/To/Cc/Bcc/첨부 없음을 보여준다.
보낸다고 한 사용자 메시지나 계정 생성만으로 발송하지 않으며 **전체 내용을 허락하고
초대 이메일 보내기**를 눌러야 한다. 임시 암호는 현재 credential과 일치하는지 검사한 뒤
서버 관리 키로 암호화해 최대15분 동안 이 확인본에 보관한다. 다른 로그인이나 관리자는
이 확인본의 암호를 복구하거나 허락할 수 없다. 만료·reset·계정 정지/삭제·암호 변경은
미발송 권한과 저장 payload를 제거한다. 복원은 미발송 초대를 만료시킨다.

같은 credential epoch에는 발송 효과가 하나뿐이며 새 UUID로 중복 발송할 수 없다.
실제 native invocation 전에 commitment를 저장하고 payload를 제거한다. **Gmail 발송
접수**는 message ID를 받았다는 뜻이며 상대 수신·열람 확인은 아니다. gws 시작 뒤 실패,
응답 유실·중단은 **발송 결과 불명**이다. People의 상태 새로 읽기는 조회만 하며 재발송하지
않는다. send-only 권한으로 Sent 확인을 만들지 않는다. 재초대하려면 명시적으로 새 암호를
발급해 이전 credential을 폐기하고 새 전체 확인을 거쳐야 한다.

실제 발송은 gws의 문서화된 native `--upload`로0600 MIME 파일을 읽는다. 본문·암호는
argv/로그에 넣지 않는다. 새0700 임시 디렉터리는 완료·오류·timeout 뒤 Rust가 제거한다.
강제 종료 중 남은 일시 평문 파일은 전용 발신자를 다음에 시작할 때 정리한다. 물리 디스크
즉시 삭제나 평문 없는 dispatch라고 주장하지 않으므로 전용 디렉터리를 보호해야 한다.
실계정 연결·scope·이메일 발송은 별도 운영자 설정과 구체적 사용자 허락으로 검증할 영역이다.
[변경 설계](../openspec/changes/admin-invitation-email/design.md)를 따른다.
