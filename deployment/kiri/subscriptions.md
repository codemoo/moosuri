# 회원 컨테이너에서 무수리 엔진 호출하기

회원 A와 B는 각각 자신의 컨테이너에서 `/model/model.sock`을 호출한다. 두 소켓의 요청을 받는 호스트 서비스는 하나의 `runtime-model`이다. 회원은 공급자 키나 실행기 주소를 입력할 필요가 없다. 엔진은 회원 UID·계정 세대·모델·예산을 확인한다. 답변은 요청한 회원에게 돌려주고 중앙 원장에는 호출 지문과 사용량을 기록한다. 대화·기억·도구 권한·작업 진행은 개인 런타임이 소유한다. [회원 소켓과 텍스트 대화 설정](README.md#공용-모델-개발판)

| 호스트 엔진 연결 | 인증과 실행 | 설치 안내 |
| --- | --- | --- |
| 설치 API 키·OpenRouter·호환 API·로컬 모델 | 엔진이 설치 설정의 모델을 직접 호출한다. | [모델 서비스 설정](README.md#공용-모델-개발판) |
| ChatGPT 공식 로그인 | 전용 워커가 검증된 사용자·워크스페이스 등록을 열고 공식 Responses API를 호출한다. 갱신 권한은 호스트의 등록 저장소에 있다. | [ChatGPT 등록 및 모델 확인](engine-chatgpt.md) |
| Claude Code | 전용 비루트 워커가 공식 CLI를 실행한다. 요청마다 같은 CLI 프로세스에서 로그인 계정과 허용 모델을 확인한 뒤 대화를 전달한다. | [Claude 로그인 및 모델 확인](engine-claude.md) |

각 엔진 구성에는 한 공급자와 모델을 지정한다. 여러 회원이 설치 인증 하나로 모델을 쓰려면 해당 용도의 API 또는 로컬 모델을 구성한다. 공식 로그인 상품은 그 공급자가 허용한 인증 주체에 연결한다. 이 모드에서 무수리 회원의 컨테이너는 같은 엔진을 계속 사용하고, 엔진이 해당 회원에게 배정된 호스트 워커를 고른다. [OpenAI 등록 조건](https://developers.openai.com/siwc/token-sharing-open-source) · [Claude Code 호스팅 조건](https://code.claude.com/docs/en/legal-and-compliance)

이 절차에는 위 워커 명령이 포함된 개발판 바이너리가 필요하다. 현재 배포판에서 제공되는 명령은 `moosuri --help`에서 확인한다.

## 로그인 기반 엔진 구성

1. 위 설치 안내에 따라 호스트에서 로그인과 모델 확인을 마친다. 각 로그인 연결에는 고유한 워커 UID·소켓·인증 경로를 배정한다. 워커 UID는 모든 회원 컨테이너의 UID와 달라야 한다.
2. 워커가 소유한 `0700` 디렉터리를 소켓 전용으로 준비한다. 루트 또는 워커가 관리하는 절대 경로를 사용한다. 워커 설정 파일은 해당 UID 소유의 `0600`으로 둔다. 아래 예시의 계정·테넌트·볼륨·세대는 설치 레지스트리의 실제 예약값으로, 모델은 로그인에서 확인한 값으로 교체한다. `adapter` 객체는 위 설치 안내의 공급자별 JSON으로 교체한다.

   ```json
   {
     "version": 1,
     "socket_directory": "/run/moosuri-engine-a",
     "owner": {
       "account_id": "11111111-1111-4111-8111-111111111111",
       "tenant_id": "22222222-2222-4222-8222-222222222222",
       "volume_id": "33333333-3333-4333-8333-333333333333",
       "generation": 1
     },
     "connection_revision": "1fc91be7-8a93-4ef1-a1cb-eee7660a601b",
     "model": "<verified-model>",
     "adapter": {"type": "claude_code", "settings": {
       "profile_file": "/srv/moosuri-claude/private/profile.json",
       "profile_digest": "<verified-profile-digest>",
       "environment_id": "<environment-uuid>",
       "expected_email_file": "/srv/moosuri-claude/private/owner-email"
     }}
   }
   ```

3. 전용 서비스 계정으로 `moosuri engine-login-worker --config /srv/moosuri-claude/private/worker.json`을 실행한다. systemd로 관리할 때는 서비스의 `User`를 그 계정으로 지정하고 `RuntimeDirectory=moosuri-engine-a`, `RuntimeDirectoryMode=0700`, `UMask=0077`로 소켓 디렉터리를 생성한다. Claude 실행기는 프로필에 지정한 rootless 엔진에 접근할 수 있어야 한다.
4. 호스트 루트 서비스의 기존 `runtime-model` 설정에서 공급자와 바인딩을 지정한다. 아래는 Claude 예시다. ChatGPT는 `subscription`에 `chatgpt_plan`, `provider.kind`에 `open_ai`를 사용한다. 회원을 추가할 때는 별도 `bindings` 항목에 해당 회원의 예약과 워커를 등록한다.

   ```json
   {
     "version": 1,
     "subscription": "claude_code",
     "connection_revision": "1fc91be7-8a93-4ef1-a1cb-eee7660a601b",
     "provider": {"kind": "anthropic", "base_url": null, "allowed_ips": [], "router_providers": [], "zero_data_retention": false},
     "model": "<verified-model>",
     "bindings": [{
       "account_id": "11111111-1111-4111-8111-111111111111",
       "tenant_id": "22222222-2222-4222-8222-222222222222",
       "volume_id": "33333333-3333-4333-8333-333333333333",
       "generation": 1, "peer_uid": 10011,
       "budget": {"max_calls": 100, "max_tokens": 250000, "reservation_tokens": 8192},
       "worker": {"socket": "/run/moosuri-engine-a/engine.sock", "worker_uid": 10111}
     }]
   }
   ```

5. 회원 설정의 모델·공급자 종류·연결 버전을 호스트 설정에 맞추고 텍스트 대화로 확인한다. 공급자 계정·실행 환경·모델을 교체할 때는 연결 버전을 새로 발급하고 회원 설정에도 적용한다. [개인 대화 확인](README.md#개인-대화에-텍스트-답변-저장하기)

엔진은 커널이 확인한 워커 소켓에서 회원과 연결 버전의 일치를 확인한 다음, 현재 회원 상태와 예산을 검사하고 같은 연결로 프롬프트를 전송한다. 워커가 꺼져 있으면 새 요청은 예산을 쓰기 전에 `model_worker_unavailable`을 반환한다. 이미 전송한 요청의 결과를 잃으면 기존 호출 ID로 영수증을 조회한다. 공급자 응답이 불확실한 요청을 새로 실행하면 추가 비용이 발생할 수 있다.
