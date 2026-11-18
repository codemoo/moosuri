# 개인 컨테이너에서 Gmail 받은편지함 연결

설치 관리자가 Google Cloud에서 Gmail API를 켜고 웹 애플리케이션 유형의 OAuth 클라이언트를 한 번 만든다. 리디렉션 URI는 개인 게이트웨이의 HTTPS origin 뒤에 `/api/v1/mail/oauth/callback`을 붙인 값이다. Google 앱이 테스트 모드면 사용할 Google 계정을 테스트 사용자에 등록한다.

## Linux 설치자 설정

기존 [개인 서비스와 HTTPS 게이트웨이](README.md)를 준비한다. 다음 항목은 호스트의 Google 브로커가 사용한다.

- Google에서 내려받은 웹 클라이언트 JSON, 브로커 구성 JSON, 서명용 난수 키를 관리자 전용 디렉터리에 보관한다. 파일은 브로커 소유, 모드 0600으로 둔다. 키 파일에는 최초 생성한 32바이트 난수를 보존한다. 키를 교체하면 기존 영수증을 사용할 수 없어 회원이 다시 연결해야 한다.
- 설치 레지스트리와 Google 소켓 루트는 각각 브로커가 접근할 수 있는 정규 절대 경로를 사용한다. 이 프로필의 브로커는 호스트 UID 0으로 실행한다. 소켓 루트는 0700으로 준비한다.
- 아래 구성에서 웹 클라이언트 경로와 공개 URI, 앱 개정 UUID, 회원의 실제 예약 값과 전용 Linux UID를 입력한다. 회원마다 서로 다른 UID를 사용한다. Google 클라이언트나 콜백을 바꿀 때는 새 앱 개정 UUID를 발급한다.

```json
{
  "app_revision": "11111111-1111-4111-8111-111111111111",
  "client_file": "/etc/moosuri/private/google-web-client.json",
  "redirect_uri": "https://assistant.example.test/api/v1/mail/oauth/callback",
  "bindings": [{
    "account_id": "22222222-2222-4222-8222-222222222222",
    "tenant_id": "33333333-3333-4333-8333-333333333333",
    "volume_id": "44444444-4444-4444-8444-444444444444",
    "generation": 1,
    "peer_uid": 10001
  }]
}
```

```sh
moosuri runtime-google \
  --registry "$REGISTRY" --socket-dir "$GOOGLE_SOCKETS" \
  --config /etc/moosuri/private/google.json \
  --capability-key /etc/moosuri/private/google-receipts.key
```

브로커가 회원별 소켓을 만든 뒤 개인 컨테이너 프로비저닝에 `--google-sockets-root "$GOOGLE_SOCKETS"`를 지정한다. 프로비저너가 해당 회원의 디렉터리 하나를 `/google:ro`로 붙이고 개인 서비스에 `--google-readonly`를 전달한다. 수명주기 구성에는 최상위 `google_sockets_root`와 해당 회원의 `enable_google_readonly`를 지정한다. 이미 실행 중인 컨테이너와 요청한 마운트가 다르면 재조정이 중단된다. 해당 회원의 프로필 교체 시점을 별도로 잡는다. Google 콜백의 쿼리에는 일시적인 인증 코드가 들어가므로 HTTPS 프록시의 요청 URI 로깅도 끈다. 제공하는 nginx 설정은 접근 로그를 끈다.

## 회원이 보는 흐름

개인 웹의 **메일 → Google로 연결하고 메일 확인**에서 본인의 Google 계정을 선택한다. 채팅에서 인식되는 최근 메일 요청도 이 화면을 연다. 이미 연결돼 있으면 받은편지함을 읽는다. 처음 연결할 때는 원래 탭과 로그인 세션으로 돌아오면 방금 동의한 계정의 메일을 한 번 조회한다. 일반 회원에게 필요한 것은 브라우저와 본인의 Google 계정이다.

요청 권한은 `gmail.readonly` 하나다. 받은편지함 첫 페이지에서 최대 다섯 건의 제목·발신자·날짜를 표시하고 다음 페이지가 있으면 화면에 알린다. 페이지 안에서는 수신 시각순으로 정렬한다. Google 토큰과 서명된 사용 권한은 개인 볼륨에 암호화해 저장한다. 화면의 연결 해제는 무수리에서의 사용을 중단한다. Google 측의 앱 접근 권한은 Google 계정에서도 철회할 수 있다. 메일 발송 권한과 발송 승인은 별도 기능이다.

이 경로에서 회원 간 격리는 원래 배정된 UID·볼륨·세대와 로그인 세션에 의존한다. 호스트 관리자에게서 실행 중인 프로세스 메모리까지 숨기는 기능은 제공하지 않는다. 시범 계정 절체 전 실제 Google 동의, Linux 마운트, 복구 후 재인증을 운영 조건에서 확인한다.
