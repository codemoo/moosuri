# 설치 관리자와 회원 초대

개인 게이트웨이가 로그인 화면과 관리 화면을 함께 제공한다. 계정의 비밀번호와 로그인 세션은 개인 볼륨에 있고, 설치 관리 권한과 회원 준비 상태는 호스트의 설치 레지스트리에 있다. 관리자가 조회할 수 있는 회원 목록에는 이메일·초대 상태·역할이 표시된다.

## 엔진과 회원별 모델

설치 관리자 화면에서 엔진의 사용 설정과 설치 기본 모델·추론 강도를 변경할 수 있다. 회원별 선택에서 다른 조합을 지정하면 해당 회원의 다음 새 호출부터 적용된다. 회원 설정을 기본값으로 되돌리면 이후의 설치 기본값 변경도 따라간다. 엔진을 일시 중지하면 새 추론을 받지 않는다. 이미 공급자에 전달된 호출은 기존 영수증으로 정산한다.

엔진이 시작될 때 호스트에 구성된 모델·강도 조합과 배정된 계정 목록을 등록한다. 관리자 화면은 이 목록을 바탕으로 선택지를 제공한다. 새 공급자의 키, 로그인 워커와 OS 권한은 호스트에서 준비한다. 연결 버전을 교체하면 이전 연결에 명시적으로 지정한 조합은 관리자가 다시 선택하거나 기본값으로 복귀시켜야 한다. [호스트 구성 예시](subscriptions.md)

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

1. **관리 → 회원 초대**에서 이름과 이메일을 입력한다. 화면에 준비 상태와 요청 번호가 표시된다. 연결이 끊긴 뒤 **초대 접수 확인**을 누르면 같은 요청 번호로 결과를 확인한다.
2. 설치자가 화면의 요청 번호로 아래 명령을 실행하고 임시 비밀번호를 수신자에게 직접 전달한다. 수신자는 처음 로그인할 때 비밀번호를 바꾼다. 웹에서 초대를 접수하는 단계에서는 이메일을 보내거나 비밀번호를 저장하지 않는다.
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

설치에서 회원의 모델·Google 브로커 바인딩까지 준비했다면 최초 실행에 각각 `--enable-text-preview`, `--enable-google-readonly`를 지정하고 재시도에도 같은 값을 쓴다. 직접 준비한 마운트를 계속 사용하는 설치에는 [개별 초대 명령](../../docs/kiri-invite.md)이 있다.
