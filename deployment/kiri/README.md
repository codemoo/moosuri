# Linux 개인 런타임 개발판

한 설치에서 회원마다 Kiri 컨테이너와 영속 볼륨을 하나씩 할당한다. 이 경로는 새 시험 설치에서 명시적으로 실행한다. 기존 설치와 PWA의 운영 버전은 [현재 상태](../../docs/status.md), 전환 조건은 [OpenSpec 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)에 있다.

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

6. 게이트웨이를 `moosuri personal-gateway --public-origin "$ORIGIN" --registry "$REGISTRY" --ipc-root "$IPC" --socket-dir "$GATEWAY_DIR"`로 시작한다. HTTPS 종단은 설치의 로컬 프록시가 담당한다. 시험용 프록시는 허용된 개인 API 경로만 전용 `gateway.sock`로 전달하고 Host·Origin을 보존한다. 클라이언트가 보낸 X-Real-IP를 버리고 관측한 클라이언트 IP로 덮어쓴다. 게이트웨이와 운영자가 개인 소켓을 열 수 있도록 실행 계정과 상위 디렉터리를 구성한다.
7. 일반 회원은 원래 로그인·비밀번호 변경 API를 사용한다. 첫 로그인은 임시 비밀번호 변경을 요구한다. 라우팅 쿠키는 게이트웨이가 서명하고 개인 서비스가 본인의 실제 세션을 검증한다. `account-runtime … fence --email "$EMAIL"`은 해당 세대의 게이트웨이 접근을 차단한다. 실행 중인 외부 작업을 중단하거나 새 세대로 복구하는 수명주기는 후속 통합 범위다.

## 검증한 경계

Ubuntu 24.04 x86_64의 격리 VM에서 실제 무수리 Linux 바이너리로 UID가 다른 회원 두 명을 초기화했다. 각 컨테이너의 준비 응답에서 계정·테넌트·볼륨·세대를 대조했다. 두 계정에 총 20개의 재조정 요청을 동시 실행도 4로 보내도 컨테이너 ID와 PID가 유지됐다. 한 볼륨의 마운트를 잃으면 그 계정의 재조정은 중단되고 다른 계정은 준비 응답을 계속 보냈다.

실제 Kiri 회원 두 명의 로그인과 비밀번호 변경이 성공했다. 양방향 쿠키 교차 제출은 HTTP 401이었고 A를 재시작해도 두 사람의 세션과 B의 프로세스가 유지됐다. 별도 게이트웨이와 개인 CLI 통합 시험에서 양쪽의 임시 비밀번호 변경, 다른 회원 쿠키 거부, 개인 서비스 강제 종료 후 세션 재사용, 같은 세대의 라우팅 차단을 확인했다. 개인 볼륨의 소유자·세대를 DB 안에서도 대조하므로 다른 이름으로 복원한 DB는 정상 시작 처리를 수행하기 전에 거부한다. 디스크 용량 산정에는 SQLite의 검사 사본과 WAL, 첨부, 저널을 포함한다. [Linux 격리 재현 절차](linux-vm-smoke/README.md)

정식 설치기의 자동 프로비저닝, 기존 운영 계정 이관, 전체 기능의 계정별 복구는 [전환 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)에서 추적한다.
