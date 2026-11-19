# Linux 개인 런타임 개발판

한 설치에서 회원마다 Kiri 컨테이너와 영속 볼륨을 하나씩 할당한다. 이 경로는 새 시험 설치에서 명시적으로 실행한다. 기존 설치와 PWA의 운영 버전은 [현재 상태](../../docs/status.md), 전환 조건은 [OpenSpec 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)에 있다.

새 설치에서는 [첫 관리자와 회원 초대](admin.md)의 명령으로 시작한다. 아래는 개별 마운트를 직접 준비하는 설치의 절차다.

## 새 시험 회원의 로그인

호스트 운영자는 비공개 설치 레지스트리에서 이메일을 정규화하고 계정·테넌트·볼륨 UUID를 발급한다. 동일 초대를 재시도할 때 원래 요청 ID를 사용한다. 비밀번호와 로그인 세션은 개인 볼륨에만 저장된다.

1. 운영자 전용(0700) 디렉터리를 만든 뒤 `moosuri account-runtime --registry "$REGISTRY" reserve --request-id "$REQUEST" --email "$EMAIL"`을 실행한다. 출력으로 받은 account_id, tenant_id, volume_id, generation을 아래에서 사용한다.
2. Linux 호스트에 Kiri 0.2.1과 검증한 무수리 OCI 이미지를 준비한다. 실행 파일은 `/usr/local/bin/moosuri`이고 이를 유일한 Entrypoint로 둔다. 프로비저너는 로컬 image config의 전체 `sha256:…`를 사용한다. 실험 이미지에는 Rust 바이너리와 해당 아키텍처의 ELF 로더·실행 라이브러리가 들어 있다.
3. 호스트 관리자가 전용 UID/GID와 크기 제한이 있는 별도 파일시스템을 할당한다. `$MOUNTS/$VOLUME`에 `nodev,nosuid,noexec`로 마운트하고 그 안에 개인 UID 소유의 0700 디렉터리 `member-data`, `scratch`, `ipc`를 만든다. 마지막 디렉터리를 `$IPC/$ACCOUNT`에 bind mount한다. 프로비저너는 IPC도 같은 한도의 파일시스템에 있어야 시작한다. 레지스트리·IPC 상위·UID 예약 디렉터리는 운영자 전용이다.
4. 개인 UID로 `moosuri personal-volume --root "$MEMBER_DATA" --account "$ACCOUNT" --tenant "$TENANT" --volume "$VOLUME" --generation "$GENERATION" initialize --name "$NAME" --email "$EMAIL"`을 실행한다. 터미널에서 임시 비밀번호를 입력한다. 설치 프로그램은 `--password-stdin`으로 비공개 파이프를 사용할 수 있다. 접수된 계정과 볼륨의 연결은 영속화되며 동일 요청으로 기존 비밀번호가 바뀌지 않는다.
5. 호스트에서 아래 명령을 실행한다. 시작 전 실제 마운트·소유자·엔진 버전·이미지와 컨테이너의 실행 구성을 확인한다. 생성 응답이 유실되면 같은 자원을 찾아 대조한다. 루트 파일시스템은 읽기 전용이고 `/data`, `/tmp`, `/ipc`만 각각 준비한 개인 경로를 사용한다. 운영 중 시작과 재시작은 호스트가 조율한다.

   ```sh
   moosuri kiri-provision --experimental --registry-directory "$REGISTRY" \
     --mounts-root "$MOUNTS" --ipc-root "$IPC" --allocations-root "$ASSIGNMENTS" \
     --email "$EMAIL" --uid "$MEMBER_UID" --image "$IMAGE_ID" --start
   moosuri account-runtime --registry "$REGISTRY" activate --email "$EMAIL" --ipc-root "$IPC"
   ```

6. 웹 배포물을 포함하는 바이너리를 빌드한다. 저장소의 `web/`에서 `npm ci && npm run build` 후 저장소 루트에서 `cargo build --locked --release -p moosuri`를 실행한다. 게이트웨이를 `moosuri personal-gateway --public-origin "$ORIGIN" --registry "$REGISTRY" --ipc-root "$IPC" --socket-dir "$GATEWAY_DIR"`로 시작한다. HTTPS 종단은 설치의 로컬 프록시가 담당한다. 회원 전용 HTTPS 호스트의 요청을 `gateway.sock`으로 전달하고 Host·Origin을 보존한다. 지원하는 화면과 API는 게이트웨이가 결정한다. 클라이언트가 보낸 X-Real-IP를 버리고 관측한 클라이언트 IP로 덮어쓴다. 게이트웨이와 운영자가 개인 소켓을 열 수 있도록 실행 계정과 상위 디렉터리를 구성한다.
7. 일반 회원은 전용 HTTPS 주소의 `/`를 연다. 이메일과 임시 비밀번호로 로그인해 비밀번호를 바꾼다. 모델이 연결된 설치에서는 첫 메시지를 보내면 대화가 만들어진다. 기억 탭에서 직접 작성한 기억과 수정 이력을 관리할 수 있다. 첫 로그인은 임시 비밀번호 변경을 요구한다. 라우팅 쿠키는 게이트웨이가 서명하고 개인 서비스가 본인의 실제 세션을 검증한다. `account-runtime … fence --email "$EMAIL"`은 해당 세대의 게이트웨이 접근을 차단한다. 실행 중인 외부 작업을 중단하거나 새 세대로 복구하는 수명주기는 후속 통합 범위다.

로그인한 회원은 개인 소켓과 게이트웨이에서 기존 API로 대화를 만들고 목록·메시지 이력을 조회하거나 대화를 삭제할 수 있다. 생성의 요청 ID 재사용, 이전 페이지 조회, 삭제 후 접근 차단과 다른 계정의 접근 거부를 합성 계정으로 확인했다.

### 비루트 HTTPS 프록시 연결

서명 키·잠금 파일은 운영자 전용 0700 디렉터리인 `--socket-dir`에 둔다. nginx 워커가 다른 사용자로 실행되면 전용 프록시 그룹을 할당하고, 게이트웨이 서비스 소유·그룹 소유권이 해당 그룹인 0710 디렉터리 `/run/moosuri-member-proxy`를 준비한다. 게이트웨이에 `--proxy-socket-dir /run/moosuri-member-proxy --proxy-gid "$PROXY_GID"`를 추가한다. 프록시는 그 안의 `gateway.sock`에만 연결한다. registry, 개인 IPC, 설치 모델 인증 파일이 있는 디렉터리는 계속 0700으로 둔다.

회원 전용 도메인의 기존 TLS server 블록에 [nginx 위치 설정](member-proxy.nginx.conf)을 포함한다. 이 예시는 nginx가 클라이언트 연결을 직접 받는 배치용이다. 로드 밸런서가 앞에 있으면 신뢰하는 프록시에서 온 주소만 nginx real-IP 모듈로 복원한다. 텍스트 응답은 최대 85초 걸릴 수 있어 예시의 upstream 대기 시간은 100초다.

호스트 부팅 때 회원을 다시 기동하려면 [Kiri 수명주기 서비스](lifecycle/README.md)에 준비된 회원을 등록한다. 서비스는 새 계정을 만들거나 정지된 세대를 활성화하지 않는다.

## 공용 모델 개발판

호스트 서비스 관리자는 모델·Google 브로커에 SIGTERM을 보내 종료할 수 있다. 모델 브로커는 진행 중인 공급자 호출을 취소하고 결과가 불확실한 호출을 원장에 남긴 뒤 소켓을 반환한다. 다음 기동은 같은 원장을 사용한다. 새 비용을 예약할 때는 요청이 시작된 뒤 회원의 정지·해제 이력이 바뀌었는지도 같은 잠금 안에서 확인한다.

설치 운영자가 한 공급자·모델·연결 버전을 지정하고, 모델 서비스만 공급자 인증 파일을 읽는다. 회원별 호출 수·토큰 예산·UID를 설정하며, 계정·테넌트·볼륨·세대는 해당 회원 예약 결과와 일치해야 한다. 각 회원에게는 전용 Unix 소켓만 보인다. API·로컬 모델과 공식 로그인 워커가 같은 회원용 텍스트 호출 경로를 사용한다. [로그인 기반 엔진 설정](subscriptions.md)

공용 모델 서비스에는 0700 레지스트리·원장·소켓 상위 디렉터리와 소유자 전용 설정 파일, 지속적으로 보관하는 32바이트 무작위 지문 키가 필요하다. 아래는 로컬 호환 모델의 설정 예시다. `peer_uid`는 Kiri가 해당 회원에게 예약한 전용 UID다.

```json
{
  "version": 1,
  "connection_revision": "1fc91be7-8a93-4ef1-a1cb-eee7660a601b",
  "provider": {
    "kind": "compatible", "base_url": "http://127.0.0.1:11434/v1",
    "allowed_ips": ["127.0.0.1"], "router_providers": [], "zero_data_retention": false
  },
  "model": "example-local-model", "key_file": null,
  "bindings": [{
    "account_id": "11111111-1111-4111-8111-111111111111",
    "tenant_id": "22222222-2222-4222-8222-222222222222",
    "volume_id": "33333333-3333-4333-8333-333333333333",
    "generation": 1, "peer_uid": 10011,
    "budget": {"max_calls": 100, "max_tokens": 250000, "reservation_tokens": 8192}
  }]
}
```

`moosuri runtime-model --registry "$REGISTRY" --ledger "$LEDGER" --socket-dir "$MODEL_SOCKETS" --config "$MODEL_CONFIG" --fingerprint-key "$FINGERPRINT_KEY"`로 서비스한다. 회원 프로비저닝에 `--model-sockets-root "$MODEL_SOCKETS"`를 추가하면 개인 런타임에 그 회원의 소켓 디렉터리만 `/model`로 읽기 전용 마운트한다. 모듈은 Unix 연결 시 커널의 peer UID와 설치 레지스트리의 세대를 검사한다.

공용 API 키를 교체할 때는 서비스 소유 디렉터리에 권한 0600인 새 파일을 준비해 같은 파일시스템에서 기존 `key_file` 경로로 원자적으로 이름을 변경한다. 이후의 새 호출은 즉시 새 키를 사용하고 이미 진행 중인 호출은 시작 때의 키로 끝난다. 파일을 읽을 수 없거나 권한이 잘못되면 새 호출에 예산을 예약하지 않는다. 기존 호출의 영수증은 계속 조회할 수 있다. 공급자·모델·접속 정책을 바꿀 때는 새 연결 버전을 발급한다.

개인 호출자는 원래 호출 ID·실행 ID·순번·모델·연결 버전을 사용한다. 동일한 요청의 중복은 정산된 영수증이나 미확정 상태를 돌려준다. 답변을 받은 회원만 그 내용을 저장한다. 전송 뒤 응답이 유실돼도 중앙에서 원문을 재생하거나 사용량을 되돌리지 않는다. 실제 제공자의 청구량이 없는 요청은 예약량을 유지한다. 설치 원장 자체의 재해 복구는 독립 수신자 검증이 남아 있다.

Linux Kiri 안의 두 실제 클라이언트에서 무수리 Rust 모델 서비스를 거쳐 로컬 합성 공급자를 호출했다. 첫 호출 2회와 서비스 재시작 뒤 새 호출 2회가 각각 한 번 정산됐다. 중복 제출은 기존 영수증, 다른 계정의 호출 ID 재사용은 충돌을 반환했다. 공급자가 받은 요청은 총 4회였고 중앙 원장에서 시험 질문과 답변 원문이 발견되지 않았다.

### 개인 대화에 텍스트 답변 저장하기

텍스트 시험 경로를 켜려면 고정 이미지에 root 소유·0444인 `/etc/moosuri/text-preview.json`을 둔다. 개인 런타임은 새 대화를 실행할 때 전용 소켓에서 자기 계정에 적용되는 선택을 조회한다. 이전 설정 파일에 모델 값이 있어도 관리자 배정이 우선한다. 게이트웨이·개인 런타임 이미지·브로커·로그인 워커는 이 프로토콜을 지원하는 같은 버전으로 업데이트한다.

```json
{"socket": "/model/model.sock", "broker_uid": 0, "max_output_tokens": 256}
```

회원 프로비저닝에 `--model-sockets-root "$MODEL_SOCKETS" --enable-text-preview`를 지정한다. 로그인한 클라이언트는 `GET /api/v1/text-preview`로 지원 여부를 확인하고, `POST /api/v1/threads/{id}/text-preview`로 `request_id`, `text`, `capability: "text_only_preview"`를 보낸다. 답변은 기존 대화 이력에서 조회한다. 응답을 잃었다면 동일 요청 ID로 기존 상태를 조회한다. 동일한 요청으로 새 호출이 발생하지 않는다. 이 API가 제공하는 기능은 텍스트 대화이며 메일 발송·웹 탐색·예약·도구가 필요한 정규 비서 요청은 기존 운영 경로를 사용한다.

## 첫 관리자와 회원 초대

최초 설치 관리자를 지정한 뒤 회원의 개인 웹에서 초대 접수, 권한 변경, 정지와 해제를 진행한다. [설치 관리자 안내](admin.md)

## 개인 Gmail 연결

개인 컨테이너에 Google 웹 클라이언트를 등록하고, 회원이 브라우저에서 동의하면 그 컨테이너의 GWS가 Gmail을 조회한다. 설치자 구성과 회원의 연결 흐름은 [개인 Gmail 연결](google.md)을 따른다.

## 개인 암호 볼트 연결 상태

`GET /api/v1/personal/vault`는 현재 개인 세션과 라우팅 세대가 유효한 회원에게 `{"state":"setup_required"}`를 반환한다. [별도 암호 볼트 서비스](../../docs/vault-service-client.md)는 기존 중앙 런타임의 전용 어댑터에서 세션 교환·검증, 쓰기 승인, 철회·삭제 전달을 처리한다. 중앙 런타임이 정지된 설치에서는 그 어댑터도 동작하지 않는다. Kiri 회원용 어댑터 연결은 준비가 필요하다.

운영자가 별도 볼트 사이트를 안내하려면 게이트웨이의 기존 `personal-gateway` 실행 명령에 `--vault-site-origin https://vault.example.test`를 추가하고 게이트웨이를 재시작한다. 예시 값은 실제 사이트의 HTTPS origin으로 바꾼다. 회원 웹과 다른 호스트를 사용하고 경로와 끝 슬래시를 생략한다. 상태 응답에는 외부 사이트 링크로 쓸 `site_origin`이 추가되며 `setup_required`가 유지된다. 사이트의 로그인은 기존 별도 인증 흐름을 따른다.

## 검증한 경계

Ubuntu 24.04 x86_64의 격리 VM에서 실제 무수리 Linux 바이너리로 UID가 다른 회원 두 명을 초기화했다. 각 컨테이너의 준비 응답에서 계정·테넌트·볼륨·세대를 대조했다. 두 계정에 총 20개의 재조정 요청을 동시 실행도 4로 보내도 컨테이너 ID와 PID가 유지됐다. 한 볼륨의 마운트를 잃으면 그 계정의 재조정은 중단되고 다른 계정은 준비 응답을 계속 보냈다. 검증한 Linux 실행 파일의 SHA-256은 `f525302d4f692a135d5ef1e628d0d5e00748932171dd4a9a9aec5f1e58db2d7e`, 해당 OCI config의 SHA-256은 `61ca38a10a8450612a4bbebb60b09b48d35efb40e8039e984ca8b13a3a4be802`이다. 이 값은 실험 당시 아티팩트를 식별한다.

실제 Kiri 회원 두 명의 로그인과 비밀번호 변경이 성공했다. 양방향 쿠키 교차 제출은 HTTP 401이었고 A를 재시작해도 두 사람의 세션과 B의 프로세스가 유지됐다. 별도 게이트웨이와 개인 CLI 통합 시험에서 양쪽의 임시 비밀번호 변경, 다른 회원 쿠키 거부, 개인 서비스 강제 종료 후 세션 재사용, 같은 세대의 라우팅 차단을 확인했다. 개인 볼륨의 소유자·세대를 DB 안에서도 대조하므로 다른 이름으로 복원한 DB는 정상 시작 처리를 수행하기 전에 거부한다. 디스크 용량 산정에는 SQLite의 검사 사본과 WAL, 첨부, 저널을 포함한다. [Linux 격리 재현 절차](linux-vm-smoke/README.md)

정식 설치기의 자동 프로비저닝, 기존 운영 계정 이관, 전체 기능의 계정별 복구는 [전환 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)에서 추적한다.
