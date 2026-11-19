# 회원별 Google Workspace 커넥터 연결

설치 관리자가 Google Cloud에서 사용할 API(Gmail, Drive, Docs, Slides, Sheets)를 켜고 웹 애플리케이션 유형의 OAuth 클라이언트를 한 번 만든다. 리디렉션 URI는 개인 게이트웨이의 HTTPS origin 뒤에 `/api/v1/mail/oauth/callback`을 붙인 값이다. Google 앱이 테스트 모드면 사용할 Google 계정을 테스트 사용자에 등록한다.

기존 설치에서 이 커넥터들을 활성화하려면 게이트웨이, GWS 릴레이와 회원 이미지를 함께 업데이트한다. 회원 DB는 스키마 87로 이전하고 기존 Gmail 권한과 진행 중인 동의 요청을 유지한다. 업그레이드 전에는 회원 쓰기를 멈추고 복원 가능한 볼륨 사본을 확보한다. 이전 바이너리는 스키마 87을 열 수 없다. 새 요청을 받기 전의 롤백은 바이너리와 이전 볼륨을 함께 복원하고, 새 쓰기를 받은 뒤에는 해당 쓰기를 보존하는 복구나 후속 버전을 사용한다.

## 회원별 GWS 설정

웹 OAuth 앱은 설치자가 한 번 준비해 여러 회원에게 제공할 수 있다. 각 회원의 Google 계정 선택, 동의 코드 교환, 토큰 갱신과 GWS 읽기는 그 회원의 컨테이너에서 처리한다. 회원이 Google Cloud 콘솔을 열거나 로컬 컴퓨터에서 로그인할 필요가 없다.

1. [Linux 개인 런타임](README.md)을 준비한다. 저장소 루트에서 `docker build -f deployment/kiri/Dockerfile -t moosuri-kiri-member .`로 Linux 이미지를 만든다. `docker save moosuri-kiri-member -o member.tar`와 `DOCKER_API_VERSION=1.47 docker --host unix:///run/kiri/docker.sock load -i member.tar`로 Kiri에 적재한 뒤 Kiri에서 확인한 이미지 ID(`sha256:…`)를 프로비저닝에 쓴다. 이미지에는 CA 인증서와 배포 파일의 SHA-256을 확인한 Google Workspace CLI 0.22.0과 각 서비스의 버전을 고정한 Google API 정의 파일이 들어간다.
2. 설치 레지스트리에서 각 회원의 예약 값과 전용 Linux UID를 확인한다. 릴레이 구성 파일에 다음처럼 등록한다. 레지스트리와 전용 소켓 루트는 별개의 root 소유 0700 디렉터리, 구성은 root 소유 0600 파일로 준비한다. 상위 디렉터리도 root가 관리해야 한다.

```json
{"bindings":[{
  "account_id":"22222222-2222-4222-8222-222222222222",
  "tenant_id":"33333333-3333-4333-8333-333333333333",
  "volume_id":"44444444-4444-4444-8444-444444444444",
  "generation":1, "peer_uid":10001
}]}
```

3. 호스트에서 릴레이를 시작한다. 릴레이는 회원 소켓에서 Google의 고정된 HTTPS 호스트로 TLS 바이트를 중계한다. Drive v3은 `www.googleapis.com`을 사용한다. 나머지 API는 `gmail`, `docs`, `slides`, `sheets`의 전용 `googleapis.com` 호스트를 사용하며, 인증에는 `oauth2`와 `openidconnect` 호스트를 사용한다. 회원별 로그인 정보는 개인 컨테이너와 Google 사이의 TLS 연결 안에서 전달된다. 게이트웨이의 Google 콜백에는 일회성 인증 코드가 들어오므로 HTTPS 프록시의 URI 로깅을 끈다. 제공하는 nginx 구성은 접근 로그를 끈다.

```sh
moosuri runtime-gws-relay --registry "$REGISTRY" --socket-dir "$GOOGLE_SOCKETS" \
  --config /etc/moosuri/private/gws-bindings.json
```

4. `kiri-onboard`에서는 설치 구성에 `google_sockets_root`를 설정하고 초대 처리 시 `--enable-local-gws`를 지정한다. 수명주기 서비스가 이 회원의 설정을 보존한다. 직접 `kiri-invite` 또는 `kiri-provision`을 실행하는 설치는 `--enable-local-gws --google-sockets-root "$GOOGLE_SOCKETS"`를 사용하고, 수명주기 설정에는 회원별 `enable_local_gws: true`를 지정한다. 컨테이너는 전용 소켓을 `/google:ro`로 마운트하고 네트워크 격리를 유지한다. 기존 컨테이너 프로필을 바꿀 때는 해당 회원의 볼륨을 보존한 채 프로필을 교체한다.
5. 각 회원 컨테이너에 해당 웹 클라이언트를 한 번 등록한다. 아래 환경 변수에는 그 회원의 예약값과 공개 HTTPS origin을 넣는다. Google에서 받은 JSON 파일은 비공개 표준 입력으로 전달한다.

```sh
docker --host unix:///run/kiri/docker.sock exec -i --user "$MEMBER_UID:$MEMBER_UID" \
  "moosuri-member-$ACCOUNT" /usr/local/bin/moosuri personal-volume --root /data \
  --account "$ACCOUNT" --tenant "$TENANT" --volume "$VOLUME" --generation "$GENERATION" \
  configure-gws --redirect-uri "$PUBLIC_ORIGIN/api/v1/mail/oauth/callback" < "$WEB_CLIENT_FILE"
```

같은 클라이언트를 다시 등록하면 기존 연결을 유지한다. ID·secret·콜백을 바꾸면 해당 회원만 다시 동의해야 한다. `gws auth login`의 데스크톱 루프백 대신 배포된 HTTPS 콜백과 PKCE를 사용한다. 개인 볼륨 전체를 재시작하면 등록과 동의가 유지된다. DB만 복구해 등록 파일이 빠진 경우에는 같은 회원에게 웹 클라이언트를 등록하고 다시 동의한다. 조회 시 컨테이너의 무수리가 현재 액세스 토큰을 짧게 실행하는 `gws` 프로세스에 전달한다. 기존 `--google-readonly` 프로필은 이전 호스트 브로커를 쓰므로 새 프로필과 서로 다른 소켓 루트를 사용한다.

## 회원이 보는 흐름

**연결**에서 사용할 서비스를 선택하고 Google 계정으로 동의한다. 설치 웹 클라이언트는 공용이고 서비스별 승인과 연결 해제는 회원의 개인 볼륨에서 처리한다. 각 카드에 해당 권한을 승인한 계정이 표시된다. Google 계정 선택 시 다른 계정을 선택할 수도 있다. 기존 Gmail 승인은 Gmail에만 적용된다.

| 공식 커넥터 | 서비스 권한 | 구현한 GWS 읽기 |
| --- | --- | --- |
| Gmail | `gmail.readonly` | 받은편지함의 최근 메일 메타데이터 |
| Google Drive | `drive.metadata.readonly` | 파일 목록, 이름, 유형 |
| Google Docs | `documents.readonly` | 지정한 문서의 제목 |
| Google Slides | `presentations.readonly` | 지정한 프레젠테이션의 제목과 슬라이드 수 |
| Google Sheets | `spreadsheets.readonly` | 지정한 스프레드시트의 제목과 탭 목록 |

Drive·Docs·Slides·Sheets는 연결한 계정을 식별하기 위해 Google의 `openid`와 이메일 권한도 요청한다. 연결 화면을 열거나 동의를 마치면 연결 상태만 확인한다. 파일의 내용 편집·생성과 공유는 이 연결의 실행 범위에 포함되지 않는다. Google Cloud 프로젝트의 동의 화면에도 사용할 범위를 등록한다. [Google Workspace의 OAuth 권한 목록](https://developers.google.com/workspace/drive/api/guides/api-specific-auth) · [GWS CLI](https://github.com/googleworkspace/cli)

채팅에서 최근 메일을 물으면 연결된 Google 계정의 받은편지함을 읽고 원래 대화에 결과를 표시한다. 첫 연결이면 **Google로 연결하고 이 대화에서 메일 확인**을 선택하고 본인의 Google 계정으로 동의한다. 같은 브라우저 세션으로 돌아오면 원래 요청을 한 번 이어간다. **연결 → Gmail** 메뉴에서 별도로 연결하면 자동 조회 없이 연결만 완료한다. 일반 회원에게 필요한 것은 브라우저와 본인의 Google 계정이다.

대화에서 조회한 결과는 개인 볼륨에 암호화해 저장한다. 같은 대화를 다시 열면 저장된 결과를 표시한다. 발신자·제목을 모델이나 도구에 넘기지 않는다. 연결을 해제하거나 다른 Google 계정으로 바꾸면 이전 결과의 암호문을 제거한다. 응답을 받지 못한 조회는 기존 접수 상태를 확인하며 임의로 Google 호출을 반복하지 않는다.

Gmail의 요청 권한은 `gmail.readonly`다. 받은편지함 첫 페이지에서 최대 다섯 건의 제목·발신자·날짜를 표시하고 다음 페이지가 있으면 화면에 알린다. 페이지 안에서는 수신 시각순으로 정렬한다. Google 토큰과 서명된 사용 권한은 개인 볼륨에 암호화해 저장한다. 화면의 연결 해제는 무수리에서의 사용을 중단한다. Google 측의 앱 접근 권한은 Google 계정에서도 철회할 수 있다. 메일 발송 권한과 발송 승인은 별도 기능이다.

이 경로에서 회원 간 격리는 원래 배정된 UID·볼륨·세대와 로그인 세션에 의존한다. 호스트 관리자에게서 실행 중인 프로세스 메모리까지 숨기는 기능은 제공하지 않는다. 시범 계정 절체 전 실제 Google 동의, Linux 마운트, 복구 후 재인증을 운영 조건에서 확인한다.
