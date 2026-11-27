# 설치 관리자와 회원 초대

개인 게이트웨이가 로그인 화면과 관리 화면을 함께 제공한다. 계정의 비밀번호와 로그인 세션은 개인 볼륨에 있고, 설치 관리 권한과 회원 준비 상태는 호스트의 설치 레지스트리에 있다. 관리자가 조회할 수 있는 회원 목록에는 표시 이름·이메일·초대 상태·역할이 표시된다. 정식 초대로 준비한 회원은 첫 로그인 전에도 초대 이름으로 검색할 수 있다. 초대 기록이 없는 기존 회원은 다음 로그인에서 확인된 이름이 등록된다.

## 초대 링크로 가입하기 — 개발 후보

새 소스 설치 후보에서는 **관리 → 회원 관리 → 회원 초대**에 이름·이메일을
입력하고 **초대 링크 만들기 → 초대 링크 복사**로 회원에게 전달한다. 메일을
자동 발송하지 않는다. 회원은 링크에서 자신이 사용할 비밀번호를 정하고
로그인한다. 개인 볼트 비밀번호는 가입 후 별도로 정한다. 현재 운영 반영과
실제 설치 수락 여부는 [현재 상태](../../docs/status.md)를 따른다.

링크는 24시간 후 만료되며 새 링크를 만들면 이전 링크는 사용할 수 없다.
관리자는 대기 중인 회원도 정지할 수 있다. 정지 해제 후에는 새 링크를
전달한다. 링크를 발급한 관리자가 권한을 잃으면 현재 관리자가 다시 발급한다.
일반적인 관리자 로그아웃은 이미 전달한 링크를 취소하지 않는다.

가입 결과가 끊기면 **상태 다시 확인**을 누른다. 이미 비밀번호가 저장됐다면
처음 정한 비밀번호로 로그인한다. 재제출로 비밀번호를 덮어쓰지 않는다.
가입 화면을 새로고침하면 전달받은 링크를 다시 열어야 한다. 비밀번호와
링크를 브라우저 저장소에 보관하지 않는다. 계속 준비 중이면 설치 운영자가
아래 UID·디스크 용량과 타이머 상태를 확인한다.

소스 설치기는 게이트웨이에 기존 온보딩 프로필·정책·기록 경로를 지정한다.
기존 설치의 업그레이드도 같은 세 경로를 명시해야 한다. 이 후보는 설치
레지스트리 v12를 사용하므로 모든 호스트 소비자를 함께 갱신한다. 웹 링크를
발급한 초대는 아래의 이전 CLI 비밀번호 입력으로 완료할 수 없다. 링크를
다시 발급해 웹에서 재개한다. [사용자 여정 J01–J08](../../openspec/changes/automate-installation-google-app/journeys.md)

## 설치 모델과 추론 강도

**관리 → 어시스턴트 엔진**에서 설치 모델과 추론 강도를 선택한다. 엔진 이용이 허용된 모든 회원의 새 모델 호출에 같은 조합을 적용한다. **서버 기본값 사용**을 선택하면 호스트에 등록된 기본 조합을 모두에게 적용한다. 이전 버전에서 저장한 회원별 선택은 업그레이드 후 적용되지 않는다. 엔진을 일시 중지하면 새 추론을 받지 않는다. 이미 공급자에 전달된 호출은 기존 영수증으로 정산한다.

사용량 조회를 도입한 버전은 설치 레지스트리를 v7로 갱신했다. 위 웹 초대 후보는 v12를 사용한다. 관리자가 여러 탭에서 같은 연결의 한도를 동시에 조회하면 진행 중인 요청을 공유한다. 조회가 끝나면 새로고침으로 다음 요청을 시작할 수 있고, 각 탭은 요청 시각부터 30초 동안 자기 요청 번호로 결과를 확인할 수 있다. v6에서 갱신하면 현재 관리자에게 속한 유효한 요청 번호를 유지한다. 배포 전 레지스트리를 백업하고 이를 여는 호스트 바이너리를 함께 갱신한다. v6 이하 전용 바이너리로 되돌리려면 호환되는 백업이 필요하다.

공개 소스 개발판의 첫 Claude 연결은 같은 화면의 **Claude 연결**에서 시작한다.
**API 키 사용**은 설치 관리자의 API 키를 확인하고 회원들이 사용할 연결을 저장한다.
**내 Claude 구독 사용**은 관리자 본인의 이메일과 호스팅 동의를 받은 뒤 필요한
실행 환경을 자동 준비하고 같은 Kiri 안에서 공식 로그인 화면을 보여준다.
별도 창이나 터미널에서 인증 주소·코드를 복사할 필요가 없다. 로그인을 시작한
세션만 화면을 보고 입력할 수 있다. 새로고침해도 같은 요청을 이어가며,
서버가 재시작되면 **중단된 설정 정리** 후 다시 시작한다.

개발판에서 **구독 연결 해제**를 누르면 이 서버의 해당 로그인을 정리한다.
완료 후 API 키 또는 새 구독을 선택할 수 있다. 중단된 정리는 같은 소유자로
로그인해 **정리 다시 시도**로 이어간다. 해제 중에는 새 연결을 시작할 수 없다.
[해제·재연결 검증](../../docs/reviews/2026-11-27-claude-disconnect.md)

이 구독 경로는 현재 개발 단계다. Linux 설치 후보에서 준비·같은 Kiri의 공식
로그인 화면·모바일/데스크톱 조작·취소·재시작·로그아웃과 다른 세션의 접근 거부,
합성 연결의 해제·재연결을 검증했다. 실제 계정 인증·추론, 인증 상태의 재부팅과
일반 회원의 개별 구독 등록은 남았다. 준비 완료나 로그인 화면 도달만으로 모델을
사용할 수 있다고 표시하지 않는다. [연결 검증과 남은 여정](../../docs/reviews/2026-11-27-claude-kiri-auth.md)

엔진이 시작될 때 호스트에 구성된 모델·강도 조합과 배정된 계정 목록을 등록한다. 관리자 화면은 이 목록을 바탕으로 선택지를 제공한다. 위 웹 연결이 제공되지 않는 공급자나 구버전 설치의 수동 구성은 별도 호스트 안내를 따른다. 연결 버전을 교체하면 이전 연결에 명시적으로 지정한 조합은 관리자가 다시 선택하거나 기본값으로 복귀시켜야 한다. [호스트 구성 예시](subscriptions.md)

관리 화면은 호스트 브로커가 마지막으로 등록한 연결 방식에 따라 API 또는 구독 사용량 페이지를 연다. OpenRouter API 연결에서는 **남은 사용량 확인**을 누르면 브로커가 공식 키 정보에서 설치 API 키의 한도와 잔여량을 조회한다. 화면의 시각은 조회 시점이다. 공급자가 초기화 주기만 주면 주기를 표시하며, 정확한 초기화 시각은 공급자가 명시했을 때 표시한다. 연결이 바뀌거나 조회가 실패하면 엔진 상태를 새로고침하고 다시 조회한다. ChatGPT·Claude 로그인과 다른 API는 해당 공식 페이지에서 확인한다. 호환 API·로컬 모델은 해당 서버 운영자에게 확인한다.

## 회원별 사용 집계

설치 운영자는 모델 서비스에 지정한 `--ledger` 디렉터리에서 로컬 호출 수와 예산에 반영된 토큰을 확인할 수 있다. sqlite3 CLI를 갖춘 호스트에서 다음 명령을 실행한다. LEDGER에는 해당 설치의 실제 디렉터리 경로를 지정한다.

```sh
sqlite3 -readonly -header -column "${LEDGER:?}/model-calls.sqlite" \
  'SELECT account_id, count(*) AS calls, sum(charged_tokens) AS budget_tokens
     FROM calls GROUP BY account_id ORDER BY account_id;'
```

계정별 예산 토큰에는 사용량을 확정할 수 없어 유지한 예약량이 포함된다. 공급자의 한도와 초기화 시각은 해당 공급자 계정에서 확인한다.

## Google Workspace

**연결 → 커넥터 → Google Workspace 설정**에서 설치용 Google 프로젝트와 API를 준비하고 웹 클라이언트를 한 번 등록한다. 이후 회원은 각자의 연결 화면에서 Google 계정만 선택하고 동의한다. [초기 설정과 회원 연결](google.md)

## 첫 관리자

새 설치는 아래의 저장공간·수명주기 설정을 먼저 준비하고 `members`를 빈 배열로 둔다. 설치자는 첫 회원의 이름과 이메일을 지정해 해당 회원의 볼륨과 로그인 화면을 준비한다. 웹 초대 접수 전이므로 이 단계에서 설치 레지스트리가 최초 회원을 예약한다. 동시에 두 설치 명령을 실행해도 한 회원만 예약된다.

```sh
sudo /usr/local/bin/moosuri kiri-onboard --experimental \
  --config /etc/moosuri/kiri-members.json \
  --request-id "$FIRST_MEMBER_REQUEST_ID" --uid "$MEMBER_UID" \
  --first-name "$ADMIN_NAME" --first-email "$ADMIN_EMAIL" \
  --operations-root /var/lib/moosuri-control/invitations
```

첫 회원이 웹에서 임시 비밀번호를 변경하면 설치자가 같은 회원의 현재 비밀번호로 아래 명령을 실행한다. `REGISTRY`와 `IPC_ROOT`는 위 설정의 `registry_directory`와 `ipc_root` 값이다. 회원 준비와 관리자 지정에는 각각 다른 UUID를 발급하고 같은 작업을 재시도할 때는 처음 사용한 UUID를 재사용한다.

```sh
moosuri installation-admin --registry "$REGISTRY" bootstrap \
  --request-id "$ADMIN_ROLE_REQUEST_ID" --email "$ADMIN_EMAIL" \
  --ipc-root "$IPC_ROOT" --member-uid "$MEMBER_UID"
```

비밀번호는 터미널 프롬프트에 입력한다. 호스트가 실제 개인 서비스에 로그인해 현재 세션을 확인한 뒤 최초 관리자 지정을 기록한다. 같은 설치에서 최초 관리자 지정은 한 번만 적용된다. 웹을 새로 열면 개인 공간에 **관리**가 나타난다.

## 회원 초대와 관리

1. **관리 → 회원 초대**에서 이름과 이메일을 입력한다. 화면에 준비 상태와 요청 번호가 표시된다. 같은 로그인 세션의 탭에서 다른 화면으로 이동하거나 새로고침한 뒤 **초대 접수 확인**을 누르면 원래 요청 번호로 결과를 확인한다. 권한 변경·정지·해제도 **변경 결과 확인**으로 같은 요청을 확인한다. 탭 저장소를 차단하면 화면을 벗어날 때 확인 정보가 사라질 수 있다.
2. 웹 가입 후보에서는 **초대 링크 만들기 → 초대 링크 복사**로 회원에게 링크를 직접 전달한다. 회원은 링크에서 자신의 비밀번호를 정하고 로그인한다. 개인 볼트 비밀번호는 가입 후 따로 정한다. 이메일 자동 발송은 하지 않는다. 구버전 운영의 서버 명령 방식은 아래에 별도로 남겨 둔다.
3. 관리 화면에서 로그인 준비가 끝난 회원을 관리자로 지정하거나 일반 회원으로 바꿀 수 있다. 계정 정지와 해제도 이 화면에서 수행한다. 정지한 계정은 게이트웨이와 공용 브로커 이용이 차단된다. 정지 해제 뒤에는 다시 로그인해야 한다. 정지 전에 열린 Google 동의 창은 새로 시작한다. 영구 차단한 런타임 세대는 정지 해제로 복구할 수 없다.

최근 권한 변경이나 세대 교체로 화면의 회원 상태가 달라지면 서버가 기존 요청을 거절하고 화면을 갱신한다. 마지막 사용 가능한 관리자는 강등하거나 정지할 수 없다. 개발판의 계정 정지에는 개인 서비스의 모든 작업 중지·세션 폐기까지 포함하는 후속 작업이 있다. [운영 전환 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)

## 초대받은 회원의 공간 준비

Linux 개발판에서 설치자는 검증된 이미지와 [수명주기 설정 파일](lifecycle/README.md)을 한 번 준비한다. 자동 저장공간 관리에는 아래 경로 구성을 사용한다. 설치 레지스트리, 작업 기록, 회원 등록 목록, 복구 서비스 상태는 서로 다른 전용 디렉터리로 둔다.

| 설정 | 경로 예시 |
| --- | --- |
| `registry_directory` | `/var/lib/moosuri-control/registry` |
| `mounts_root` | `/srv/moosuri-storage/mounts` |
| `ipc_root` | `/srv/moosuri-storage/ipc` |
| `allocations_root` | `/srv/moosuri-storage/assignments` |
| `enrollment_directory` | `/var/lib/moosuri-control/enrolled` |
| `--operations-root` | `/var/lib/moosuri-control/invitations` |

최초 사용 전에 비어 있는 저장공간 루트 `/srv/moosuri-storage`, 등록 목록과 작업 기록 디렉터리를 root 소유 0700으로 준비한다. 저장공간의 정확한 권한·용량 검사와 장애 복구 방법은 [저장공간 운영 안내](storage.md)를 따른다. 각 상위 디렉터리는 root가 소유하고 그룹·기타 쓰기 권한이 없어야 한다. 회원 계정으로 초기화 프로세스를 실행하므로 저장공간 루트까지 경로 탐색이 가능해야 한다. `max_volume_bytes`만큼 실제 디스크 공간을 준비한다. 호스트에 등록된 사용자·그룹이나 다른 회원에 배정한 UID는 사용할 수 없다.

```sh
sudo /usr/local/bin/moosuri kiri-onboard --experimental \
  --config /etc/moosuri/kiri-members.json \
  --request-id "$INVITATION_REQUEST_ID" --uid "$MEMBER_UID" \
  --operations-root /var/lib/moosuri-control/invitations
```

명령은 웹 초대에서 이름·이메일을 읽고, 해당 회원의 볼륨과 IPC를 마운트한 뒤 재부팅 복구 목록에 등록한다. 터미널에서 요청할 때 임시 비밀번호를 입력한다. `ready`는 지정한 회원 프로세스의 소켓으로 준비 상태를 확인하고 라우팅을 활성화했다는 뜻이다. 중단되면 같은 요청 번호·UID·설정으로 재실행한다. 처음 설정한 비밀번호가 이미 저장됐으면 다시 입력할 필요가 없다.

설치에서 회원의 모델 브로커와 [회원별 GWS 릴레이](google.md) 바인딩까지 준비했다면 최초 실행에 각각 `--enable-text-preview`, `--enable-local-gws`를 지정하고 재시도에도 같은 값을 쓴다. 직접 준비한 마운트를 계속 사용하는 설치에는 [개별 초대 명령](../../docs/kiri-invite.md)이 있다.

## 구버전의 웹 초대 자동 준비와 서버 명령

Linux 설치 운영자는 [서비스](lifecycle/moosuri-kiri-onboard.service)와 [타이머](lifecycle/moosuri-kiri-onboard.timer)를 선택적으로 켤 수 있다. 켜면 설치 관리자가 웹에서 접수한 **기존 대기 초대를 포함해** 최대 16건씩 순서대로 검사한다. 작업자는 설치 설정에 지정한 회원 이미지·저장공간·릴레이를 사용한다. 관리자가 최초 비밀번호를 입력할 때까지 새 회원은 로그인할 수 없다.

1. 테스트용 Linux 설치에서 먼저 위의 수동 온보딩 경로와 Kiri 엔진을 준비한다. 이 설치만 사용할 전용 UID/GID를 배정한다. 시스템 계정, 다른 설치, 수동 온보딩용 UID와 겹치지 않게 한다.
2. 아래 예처럼 root 소유 0600 파일 `/etc/moosuri/kiri-onboard-policy.json`을 만든다. `uid_pool`의 크기가 자동 등록 슬롯의 상한이고 `max_total_volume_bytes`는 작업자가 예약하는 모든 볼륨의 합계 상한이다. 예시는 회원당 10 GiB, 최대 2명이다. 기존 회원의 디스크 사용량을 제외하고 이 용량을 확보한다. `enable_text_preview`를 켜려면 모델 브로커 경로도 설정한다.
3. 아래 명령으로 한 번 실행한다. 셸에서 실행해도 일반 검사에는 비밀번호를 입력하지 않는다. 합성 회원의 요청 번호·단계를 확인한 뒤 타이머를 설치할 수 있다.

```json
{"uid_pool":[41001,41002],"max_total_volume_bytes":21474836480,"enable_text_preview":false}
```

```sh
sudo /usr/local/bin/moosuri kiri-onboard-worker --experimental \
  --config /etc/moosuri/kiri-members.json --policy /etc/moosuri/kiri-onboard-policy.json \
  --operations-root /var/lib/moosuri-control/invitations
sudo install -m 0644 deployment/kiri/lifecycle/moosuri-kiri-onboard.{service,timer} /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now moosuri-kiri-onboard.timer
sudo journalctl -u moosuri-kiri-onboard.service -n 30
```

`password_required`이면 운영자가 요청 번호를 대조하고 아래 명령을 실행한다. 숨은 터미널 프롬프트에 임시 비밀번호를 입력한다. 이미 준비한 운영자 전용 비밀번호 파일을 사용하는 경우에만 마지막에 `--password-stdin < /root/private/member-password`를 붙여 root 셸에서 실행한다. 한 줄 끝에 개행이 필요하다. 자격 증명 전달은 운영자가 담당한다. 작업자는 생성·발송하지 않는다.

```sh
sudo /usr/local/bin/moosuri kiri-onboard-worker --experimental \
  --config /etc/moosuri/kiri-members.json --policy /etc/moosuri/kiri-onboard-policy.json \
  --operations-root /var/lib/moosuri-control/invitations --finish-request "$INVITATION_REQUEST_ID"
```

`ready`가 출력되면 해당 회원 소켓의 준비와 라우팅 활성화가 끝났다. 중단되면 같은 명령을 재실행한다. 작업 기록 디렉터리의 `web-onboarding.json`에는 작업자가 배정한 요청 번호·UID·볼륨·프로필이 저장된다. 작업 기록·등록 목록·설치 레지스트리·저장공간 기록은 한 설치의 세트로 보존한다. 예약 후 이미지나 경로가 달라지면 해당 요청은 멈춘다. 완료 전에는 원래 설정을 복원해 재시도한다. 수동으로 준비를 시작한 회원은 앞 절의 기존 요청 번호·UID·명령을 계속 사용한다.

`capacity_waiting`은 UID 또는 디스크 예산이 소진됐다는 뜻이다. 슬롯은 회원 정지나 타이머 중지 후에도 예약 상태로 남는다. 새 용량과 UID를 준비한 뒤 예산을 늘릴 수 있다. `onboard_operation_in_progress`는 같은 작업 디렉터리를 쓰는 다른 온보딩이 실행 중이라는 뜻이다. 자동 준비를 중단하려면 `sudo systemctl disable --now moosuri-kiri-onboard.timer`를 실행한다. 진행 중인 서비스는 현재 검사를 마친다.

Ubuntu 24.04.5·Kiri 0.2.1 일회용 게스트에서 웹 초대 2건을 등록해 타이머 실행, 한도 도달 시 대기, 비밀번호 입력 후 첫 로그인, 재실행 시 기존 비밀번호 보존과 증설 후 두 번째 회원 준비를 확인했다. [합성 설치 검증 기록](linux-vm-smoke/onboard-worker-evidence.json)
