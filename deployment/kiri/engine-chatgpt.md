# Kiri 엔진의 ChatGPT 공식 로그인 연결

이 연결은 한 ChatGPT 사용자와 그 사용자가 승인한 워크스페이스의 등록을 하나의 Kiri 계정에 연결한다. 회원의 무수리 로그인은 기존 개인 런타임에서 처리된다. 엔진은 전용 비루트 워커의 자격 증명 저장소에서 SIWC 세션을 열고, 같은 등록의 공식 모델 목록에서 선택해 텍스트 연결을 확인한 모델을 호출한다. 관리형 호스팅 또는 고객 과금이 포함된 배치에는 OpenAI가 안내하는 별도 등록 조건을 적용한다. [OSS SIWC 안내](https://developers.openai.com/siwc/token-sharing-open-source) · [추론 규약](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)

## Linux에서 전용 저장소 준비

아래 예시는 엔진 워커의 OS 계정이 `moosuri-engine-chatgpt`이고 설치한 무수리 바이너리가 `/usr/local/bin/moosuri`인 경우다. 배치마다 전용 UID와 저장소를 지정한다. 초기화·등록 서버·워커·검사는 같은 비루트 UID로 실행한다. 저장소 디렉터리와 로컬 암호는 이 UID가 소유하고, 부모 경로는 루트 또는 이 UID가 관리한다. 회원 볼륨·백업·설치 DB와는 별개의 경로를 사용한다.

```sh
sudo install -d -m 0700 -o moosuri-engine-chatgpt /var/lib/moosuri-engine-chatgpt
sudo -u moosuri-engine-chatgpt sh -c '
  umask 077
  set -C
  openssl rand -base64 32 > /var/lib/moosuri-engine-chatgpt/password
'
sudo -u moosuri-engine-chatgpt /usr/local/bin/moosuri \
  --data-dir /var/lib/moosuri-engine-chatgpt/store init \
  --name "ChatGPT enrollment" --email engine@example.com \
  --password-file /var/lib/moosuri-engine-chatgpt/password
```

`password`는 등록용 무수리 계정의 암호다. 등록하는 사용자에게는 보호된 방법으로 이 로컬 암호를 제공한다. 공급자의 자격 증명은 SIWC와 보호된 SSH 인계가 생성한 데이터로 전용 저장소에 암호화된다.

## 브라우저 로그인과 모델 확인

1. 브라우저 컴퓨터와 Linux 호스트에서 접근할 수 있는 전용 HTTPS 주소를 준비하고, 정상 TLS 검증이 가능한 사설 프록시로 같은 호스트의 `127.0.0.1:43210`에 연결한다. 예시의 `engine-enroll.example.com`은 이 주소로 바꾼다. 등록을 진행하는 사용자와 SSH 인계가 끝날 때까지만 접근을 연다.
2. Linux 호스트에서 전용 등록 서버를 실행한다.

   ```sh
   sudo -u moosuri-engine-chatgpt /usr/local/bin/moosuri \
     --data-dir /var/lib/moosuri-engine-chatgpt/store serve \
     --bind 127.0.0.1:43210 --origin https://engine-enroll.example.com
   ```

3. 브라우저 컴퓨터에서 다음 명령을 실행한다. `kiri-enrollment`은 Linux 호스트의 신뢰된 SSH 별칭이다. 해당 SSH 세션의 PATH 또는 `~/.local/bin`에 같은 버전의 무수리 바이너리가 있어야 한다. SSH 수신기는 등록 서버의 소유자 API에 보호된 인계를 전달한다.

   ```sh
   moosuri subscriptions chatgpt-vm \
     --server https://engine-enroll.example.com \
     --email engine@example.com --ssh kiri-enrollment
   ```

   등록용 무수리 암호를 입력하고 브라우저에서 실제로 이 Kiri 계정을 사용할 ChatGPT 사용자와 워크스페이스로 동의한다. 브라우저 컴퓨터가 OAuth를 완료하며 Linux 저장소에는 Linux 호스트 ID가 보존된다. [개인 VM 인계 절차](../../docs/chatgpt-authentication.md#personal-self-hosted-linux-transfer) · [공식 VM 안내](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms)

4. 같은 HTTPS 주소에 등록용 무수리 계정으로 로그인한다. 설정의 **내 ChatGPT**에서 모델 목록을 불러오고, 엔진에 연결할 모델을 선택해 **텍스트 연결 확인**을 실행한다. 사용할 모델과 추론 강도 조합마다 텍스트 연결을 확인한다. 확인이 끝나면 등록 서버를 중지하고 프록시 경로를 닫는다.
5. Linux 호스트에서 아래 명령으로 계정·등록 리비전·모델을 확인한다. 출력의 `choices`에서 사용할 모델과 추론 강도를 함께 확인한다. 조합이 없으면 등록 서버에서 해당 모델·강도의 텍스트 연결을 확인한다. `models`는 기본 강도로 확인한 모델 목록이다. 검사는 로컬에 저장된 검증 결과를 출력한다.

   ```sh
   sudo -u moosuri-engine-chatgpt /usr/local/bin/moosuri engine-chatgpt-inspect \
     --store-dir /var/lib/moosuri-engine-chatgpt/store \
     --enrollment-email engine@example.com \
     --password-file /var/lib/moosuri-engine-chatgpt/password
   ```

## 워커 설정

워커 설정의 `model`에는 검사 결과에 나온 정확한 모델을 입력한다. `adapter.settings.enrollment_account`, `binding`, `revision`도 같은 결과에서 복사한다. 허용할 Kiri 계정의 식별자는 엔진 연결 경로의 별도 설정에서 지정한다.

```json
{
  "type": "chatgpt_plan",
  "settings": {
    "enrollment": {
      "store_dir": "/var/lib/moosuri-engine-chatgpt/store",
      "enrollment_email": "engine@example.com",
      "password_file": "/var/lib/moosuri-engine-chatgpt/password"
    },
    "enrollment_account": "11111111-1111-4111-8111-111111111111",
    "binding": "22222222-2222-4222-8222-222222222222",
    "revision": 1
  }
}
```

워커는 전용 저장소의 쓰기 잠금을 보유한다. 같은 저장소에서 등록 서버나 두 번째 워커를 동시에 실행할 수 없다. 워커가 다시 연결을 요구하면 먼저 중지하고, 등록 서버의 기존 등록을 선택해 `chatgpt-vm`에 `--binding ID --revision N`을 추가한다. 새 리비전에서 모델 목록과 텍스트 연결을 확인한 뒤 워커의 등록 리비전을 갱신한다. 엔진과 워커에도 같은 새 연결 리비전을 지정한다. OpenAI의 토큰 갱신 결과를 저장소에서 확정할 수 없는 경우 다시 인증한다. 운영 중 연결을 종료하려면 워커를 중지하고 등록 서버에서 연결을 해제한다. 인계된 세션의 공급자 쪽 해제는 ChatGPT의 앱 연결 설정에서도 확인할 수 있다.
