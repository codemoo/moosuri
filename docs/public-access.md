# 도메인으로 공개 셀프호스팅 접속

서버는 본인의 컴퓨터/VPS에서 실행한다. 접속 방식은 **Tailscale** 또는 **내 도메인으로
공개 접속**이다. 네트워크를 나중에 연결하려면 설치에서 로컬 사용을 먼저 고를 수 있다.

| 방식 | 접속 조건 |
| --- | --- |
| [Tailscale](tailscale.md) | 서버와 접속 기기가 같은 tailnet에 연결됨 |
| 공개 셀프호스팅 | 사용자가 미리 등록한 DNS와 외부 접속 경로가 이 서버에 도달함 |

무수리는 DNS 제공자와 관계없이 **이미 등록된 도메인을 확인해서 연결**한다. DNS 기록을
생성·수정하거나 DNS API 토큰을 요구하지 않는다.

## 연결 순서

1. 도메인의 DNS를 서버 주소로 등록하고 외부 TCP443이 서버로 전달되게 준비한다.
2. 설치에서 **공개 셀프호스팅 · 내 도메인**을 고르거나 `moosuri public connect`를 실행한다.
3. 무수리가 DNS를 조회하고 임시 확인 주소를 만든다. 정상 TLS로 돌아온 응답이 이 서버의
   일회용 값과 일치해야 연결을 허용한다. 다른 서버·잘못된 주소·인증서 오류는 통과하지 않는다.
4. 확인에 성공한 뒤에만 무수리 접속 주소를 전환하고 서버를 시작한다. 계정·대화·자료는 유지한다.

확인은 최대2분이다. 확인용 경로는 비서 데이터를 제공하지 않으며 검사 후 종료한다.
중단되면 `connect`로 재개하거나 `disconnect`로 이전 주소를 복원한다.

```sh
moosuri public connect
moosuri public status
moosuri public disconnect
```

Tailscale과 공개 관리형 연결은 하나씩 사용한다. 바꾸려면 현재 연결의 `disconnect`를
먼저 실행한다. 주소가 바뀌면 새 주소에서 로그인·Push를 등록하고 Google OAuth callback을 갱신한다.

## HTTPS 방식

**무수리에서 인증서 자동 발급·갱신**은 별도 프록시 없이 Rust 서버가 Let's Encrypt
TLS-ALPN-01을 수행한다. [가입자 약관](https://letsencrypt.org/repository/)과 인증서 도메인
이름 공개가 적용된다. 인증서와 account 키는 SQLite 자료 폴더 밖의 비공개 sibling
`.moosuri-https-<data-path-hash>`에0600으로 원자 저장하고 재시작 시 재사용한다.
일반 DB 백업에는 포함되지 않으며 필요하면 별도 비공개 백업 또는 CA 재발급을 이용한다.
서버 중지 시 갱신도 멈추며 서버 실행 중 자동 갱신한다.

```sh
moosuri public connect --domain assistant.example.com --tls-port 8443 --no-browser
```

native 모드는 IPv4 A와 **외부443 → 선택한 TLS 포트**가 필요하다. 기본8443은 root 없이
사용한다.443 직접 바인딩 권한은 운영자가 준비하거나 기존 프록시를 쓴다. 공유기·CGNAT·
방화벽은 DNS만으로 해결되지 않는다. 기존443 서비스를 덮어쓰거나 sudo/라우터 설정을
실행하지 않는다. Cloudflare를 쓰면 native TLS-ALPN이 도달하도록 DNS-only로 설정한다.

**기존 Nginx/Caddy/HTTPS 프록시 사용**은 현재 프록시가 인증서를 관리한다. 내부 주소는
`127.0.0.1:설치한_포트`로 지정한다. 기존 설정 파일은 자동 변경하지 않는다.

```sh
moosuri public connect --domain assistant.example.com --proxy --no-browser
```

Caddy 예시(내부 포트를 바꿨으면3210을 변경):

```caddyfile
assistant.example.com {
    reverse_proxy 127.0.0.1:3210 {
        header_up X-Real-IP {remote_host}
    }
}
```

프록시 모드는 loopback 프록시가 `X-Real-IP`를 실제 소켓 주소 하나로 덮어써야 한다.
기존 사설 HTTPS/split-DNS 설치에는 이번 공개 모드의 DNS 제약을 소급 적용하지 않는다.

## 상태와 제한

`public status`는 DNS·로컬 서버·정상 TLS HTTPS 응답을 구분한다. 도메인 확인은 현재
서버에서 수행하므로 모든 외부 네트워크의 접근성까지 보장하지 않는다. NAT loopback이
없는 환경에서는 이 서버에서 도메인에 돌아올 수 있도록 네트워크를 구성해야 한다.
휴대폰 데이터망 같은 다른 네트워크에서 `/app` 로그인까지 최종 확인한다.

연결 해제·runtime 제거 시 데이터와 인증서 키를 유지한다. DNS는 사용자가 관리한다.
실제 CA 발급·공유기·외부 기기 수락과 합성/로컬 TLS 검증은 구분한다.
[검증 기록](reviews/2026-11-17-public-access.md)을 따른다.

참고: [Let’s Encrypt challenge](https://letsencrypt.org/docs/challenge-types/),
[rustls-acme0.15.4](https://docs.rs/rustls-acme/0.15.4/rustls_acme/),
[Caddy 자동 HTTPS](https://caddyserver.com/docs/automatic-https).
