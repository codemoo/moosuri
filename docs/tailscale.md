# Tailscale로 어디서나 무수리 사용하기

무수리 서버와 접속할 휴대폰/컴퓨터를 Tailscale에 연결하면 같은 네트워크에서
`https://기기이름.네트워크.ts.net/app`으로 접속하고 PWA를 설치할 수 있다.
무수리 계정 로그인은 그대로 필요하다. Tailscale은 접속 경로이며 계정을 대신하지 않는다.

## 설치와 연결

1. 서버에 [공식 Tailscale](https://tailscale.com/download)을 설치하고 실행한다.
2. 무수리 설치에서 **Tailscale · 내 기기끼리 접속**을 고른다. 기기 주소를 자동으로
   읽고, 필요한 경우 브라우저 로그인을 안내한다. 계정 설정 뒤 전용 HTTPS 연결을 만든다.
3. Tailscale 네트워크에서 HTTPS가 꺼져 있으면 [DNS 설정](https://login.tailscale.com/admin/dns)에서
   HTTPS 인증서를 켜고 다시 실행한다. 인증서 이름 공개에 대한 Tailscale 안내를 확인한다.
4. 접속할 기기도 같은 Tailscale 네트워크에 연결하고 출력된 주소에서 무수리에 로그인한다.

이미 설치했다면 다음 명령을 쓴다. 자동 연결을 지원하는 버전으로 업데이트한 뒤 실행한다.

```sh
moosuri tailscale connect
moosuri tailscale status
moosuri tailscale disconnect
```

`connect`는 기존 데이터와 계정을 유지하면서 접속 주소를 바꾸고, 등록된 사용자 서비스가
있으면 시작/재시작한다. 로그인 시 자동 시작 설정은 유지한다. 터미널에서 직접 실행 중인
서버는 Ctrl+C로 중지한 뒤 연결하고 `moosuri open --start`로 다시 시작한다.
`connect --no-browser`는 브라우저 자동 열기를 생략한다. 최초 Tailscale 로그인 URL은
연결된 터미널에서만 표시하며, 터미널이 없으면 Tailscale 앱에서 먼저 로그인한다.

주소가 바뀌면 새 주소에서 다시 로그인하고 기기 알림을 다시 켠다. Google OAuth를 쓰면
관리자 화면에 표시되는 새 callback 주소를 해당 Google 앱에도 등록해야 한다.
`status`는 기기 연결, 무수리가 관리하는 Serve 경로, 실제 HTTPS 응답을 구별한다.
설정 성공만으로 DNS/인증서/다른 기기의 접근까지 완료됐다고 표시하지 않는다.

`disconnect`는 무수리가 만든 변경되지 않은 경로만 제거하고 이전 무수리 접속 주소와
배포 예시를 복구한다. Tailscale 자체를 로그아웃하거나 종료하지 않는다. 다른 서비스의
경로, Funnel과 수동 변경은 보존한다. 무수리 제거도 관리 중인 연결을 같은 방식으로
정리하므로 Tailscale 앱이 실행 중이어야 한다. 다른 Tailscale 프로필로 바꿨다면 원래
프로필에서 연결을 해제한 뒤 원하는 프로필로 다시 연결한다.

## 플랫폼과 문제 해결

- **macOS:** App Store/Standalone 앱의 인증된 로컬 API와 open-source daemon의 Unix
  socket을 지원한다. Standalone 앱은 OS 계정의 기존 admin 권한에 따라 접근이 제한될 수 있다.
- **Linux:** 실행 중인 tailscaled의 `/var/run/tailscale/tailscaled.sock`을 사용한다.
  권한이 없으면 Tailscale의 operator 설정을 진행한다. 무수리는 sudo를 실행하지 않는다.
- **WSL2:** Tailscale은 [Windows 호스트 설치를 권장](https://tailscale.com/docs/install/windows/wsl2.md)한다.
  WSL 내부에서 Windows daemon을 자동 제어하거나 두 번째 daemon을 설치하지 않는다.
  설치 메뉴의 **HTTPS 주소 직접 입력 (WSL/고급)**에서 Windows Tailscale의 주소를 입력하고,
  Windows에서 아래의 Serve 경로를 직접 설정한다. 먼저 Windows의 `127.0.0.1:PORT`가
  해당 WSL 서버로 전달되는지 확인해야 한다. WSL 종료 시 서버도 종료된다.

```sh
# Manual mode only: review an existing Serve configuration before changing it.
tailscale serve --bg --https=443 --set-path=/ http://127.0.0.1:3210
```

기존 HTTPS 443 연결이 있으면 자동으로 덮어쓰지 않는다. 다른 서비스와 같은 origin을
공유하지 않도록 전용 기기를 쓰거나 기존 구성을 직접 조정한다. 중간에 다른 설정이
변경되면 재시도 안내가 나오며 기존 설정을 보존한다. 연결이 중단됐다면 `connect`로
이어가거나 `disconnect`로 이전 접속을 복구한다. 소프트웨어 업데이트/시작은 진행 중인
연결 변경을 마친 뒤 가능하다. 별도 socket은 `moosuri tailscale --socket /absolute/path ...`로
지정할 수 있다. 연결된 socket 경로는 기억하므로 상태 확인·해제·제거 때 다시 입력할 필요가 없다.

## 구현·검증 경계

공식 Tailscale **v1.104.7**의 LocalAPI를 기준으로 구현했다. peer 목록을 요청하지 않고,
Serve 설정 전체를 보존하면서 원래 ETag를 `If-Match`로 전송한다. 동시 변경은 412로
거절한다. CLI의 단순 `serve` 재실행은 사전 검사와 CLI 내부 읽기 사이의 변경을
덮어쓸 수 있어 자동 연결 경로에는 사용하지 않는다. 루트 경로의 소유 기록은 설치
디렉터리에 비공개로 저장하며, 동일한 경로가 있어도 소유 기록 없이는 채택하지 않는다.

macOS 로컬 API 토큰은 제한된 메모리에서만 사용하며 저장/출력하지 않는다. 설치 기록에는
토큰·AuthURL·peer 목록을 넣지 않는다. API/구성 형식이나 ETag를 확인할 수 없으면 변경하지
않는다. LocalAPI는 상위 서비스의 비안정 API이므로 향후 호환성 검증이 필요하다.
실제 계정 로그인·인증서 발급·기기 간 HTTPS 및 각 macOS 배포판/WSL 실기기 검증은
합성 daemon/CLI 검증과 구분한다.

참고: [Serve](https://tailscale.com/docs/reference/tailscale-cli/serve),
[Serve CAS](https://github.com/tailscale/tailscale/blob/v1.104.7/client/local/serve.go),
[상태·로그인 API](https://github.com/tailscale/tailscale/blob/v1.104.7/client/local/local.go),
[macOS 연결 방식](https://github.com/tailscale/tailscale/blob/v1.104.7/safesocket/safesocket_darwin.go).
