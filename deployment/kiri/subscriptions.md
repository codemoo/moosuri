# 무수리 엔진과 모델 공급자 연결

설치자는 호스트에서 무수리 엔진의 모델 연결을 관리한다. 회원 A와 B의 컨테이너에는 각각 자신의 `/model/model.sock`이 보인다. 호스트에서는 두 소켓이 같은 무수리 프로세스(`runtime-model`)로 연결된다. 엔진이 호출자의 UID와 현재 계정 상태를 확인하고, 설정된 공급자에 추론을 요청한 뒤 응답과 사용량을 해당 호출에 연결한다. 대화 이력·기억·도구 권한·작업 진행은 회원의 개인 런타임이 관리한다. [엔진 구현](../../apps/moosuri/src/runtime_model.rs) · [컨테이너 구성](README.md)

| 엔진의 모델 연결 | Kiri 개발판 지원 범위 |
| --- | --- |
| 설치 API 키, OpenRouter, 호환 API, 로컬 모델 | 회원 전용 소켓을 통한 텍스트 추론. 두 컨테이너에서 동일한 엔진을 호출하는 경로를 검증했다. |
| ChatGPT 공식 로그인 | 기존 로그인·추론 경로를 Kiri 엔진에 연결하는 작업이 남아 있다. 설정값 `chatgpt_plan`의 현재 진단은 `chatgpt_engine_adapter_unavailable`이다. |
| Claude 공식 실행기 | Linux 비루트 실행기와 호스트 엔진 사이의 연결이 필요하다. 설정값 `claude_code`의 현재 진단은 `claude_rootless_executor_bridge_required`이다. |

회원의 무수리 로그인과 모델 공급자의 로그인은 별개다. 공급자 계정의 사용 범위는 엔진 뒤에 연결한 서비스의 공식 인증 방식과 계약에 따른다. 별도의 공급자 인증이 필요한 연결은 해당 어댑터의 설정에서 안내한다.

## 공식 지원 범위

| 경로 | 확인한 근거와 판단 |
|---|---|
| ChatGPT SIWC | [OSS overview](https://developers.openai.com/siwc/token-sharing-open-source)는 issued `client_id`를 인증한 사용자와 선택 workspace에 묶는다. 같은 사용자의 여러 host는 가능하지만 다른 사용자의 가족/회원 풀로 쓰라는 허가는 아니다. 유료 또는 remotely hosted 앱은 별도 interest/등록 경로를 안내한다. |
| 개인 self-hosted VM | [Self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms)는 같은 사용자·workspace의 로컬 OAuth, 보호된 credential 전달, VM 고유 host ID 보존과 VM의 refresh 소유를 설명한다. 이전한 session의 host별 usage attribution/revocation은 아직 제공되지 않는다. 이 절차를 설치자 구독의 다중 회원 공유로 확대하지 않는다. |
| SIWC inference | [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)는 공개 `api.openai.com/v1/models`와 `/responses`, account별 모델 목록, `store:false`, `stream:true`, terminal `response.completed`를 요구한다. [Preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)는 HTTP `previous_response_id`, `max_output_tokens`와 여러 hosted tool을 지원하지 않는다. ChatGPT `backend-api`, Codex 로그인 파일 추출, scope 위장은 대안이 아니다. |
| 자체 Claude 제품/SDK | [공식 legal/compliance](https://code.claude.com/docs/en/legal-and-compliance)의 Authentication and credential use는 자체 제품/서비스와 Agent SDK에 Console API key 또는 지원 cloud를 안내한다. consumer Claude OAuth를 Rust HTTP 모델 adapter로 전달하는 기능은 만들지 않는다. |
| 변경 없는 Claude Code hosting | 같은 문서의 Can customers offer Claude Code in their products?는 별도 합의가 없으면 Commercial Terms 수락, 게시된 binary의 무변경 실행, 내장 인증 방식 유지, 각 최종 사용자의 자체 인증·직접 과금 등을 요구한다. “Customers may not pay for, resell, or intermediate Claude usage on their end users’ behalf.” 설치자의 Claude 구독을 모든 회원의 공용 사용량으로 중계하는 경로는 제공하지 않는다. |

공식 문서는 해당 설치·계정·모델의 사용 자격이나 계약 수락 증명이 아니다. 이 작업은 인증·결제·실제 provider 추론을 수행하지 않았다.

## 엔진에 어댑터를 연결할 때

- ChatGPT 어댑터는 기존 공식 로그인 구현의 소유자·워크스페이스 확인과 갱신 잠금을 재사용한다. [로그인 구현](../../crates/moosuri-agent/src/subscriptions.rs) · [인증 저장소](../../crates/moosuri-store/src/subscriptions.rs)
- Claude 실행기는 비루트 Linux UID를 요구한다. 호스트 엔진에서 해당 실행기에 요청을 넘기고, 검증된 프로필과 인증 환경을 지정해야 한다. [실행 조건](../../docs/claude-execution.md) · [프로필 검사](../../crates/moosuri-connectors/src/claude_code/sandbox.rs)
- 어댑터 수락 시험은 두 컨테이너가 같은 엔진에 동시에 요청하는 흐름에서 시작한다. 각 응답의 소유자, 공급자 세션 분리, 단일 갱신, 재시도 시 중복 호출·정산 방지를 확인한다. [전환 작업표](../../openspec/changes/isolate-account-runtime/tasks.md)
