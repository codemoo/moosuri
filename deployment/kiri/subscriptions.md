# Kiri 구독 모델의 현재 경계

2026-11-19에 공식 자료와 현재 코드를 대조했다. **현재 `runtime-model`은 API/compatible/local 모델 broker다. Kiri용 ChatGPT/Claude 구독 실행기는 구현되지 않았다.** 기존 단일 사용자 프로그램의 로그인·실행 기능이나 합성 child 성공을 Kiri 멀티유저 수락으로 표시하지 않는다.

`runtime-model` 설정에 명시적으로 `"subscription":"chatgpt_plan"`을 넣으면 `chatgpt_shared_subscription_unsupported`, `"subscription":"claude_code"`를 넣으면 `claude_rootless_executor_bridge_required`로 시작을 거절한다. 두 검사는 provider credential, fingerprint key, 호출 원장, 소켓 및 외부 실행기를 열기 전에 끝난다. 기존 API 설정은 이 필드를 생략한다. API 요청·fingerprint·정산·epoch 검사는 바뀌지 않는다. 이 표시는 token 종류를 추측하는 검사가 아니다. `key_file`에는 API/local credential만 사용하며 개인 구독 token을 API key로 복사하지 않는다.

## 공식 지원 범위

| 경로 | 확인한 근거와 판단 |
|---|---|
| ChatGPT SIWC | [OSS overview](https://developers.openai.com/siwc/token-sharing-open-source)는 issued `client_id`를 인증한 사용자와 선택 workspace에 묶는다. 같은 사용자의 여러 host는 가능하지만 다른 사용자의 가족/회원 풀로 쓰라는 허가는 아니다. 유료 또는 remotely hosted 앱은 별도 interest/등록 경로를 안내한다. |
| 개인 self-hosted VM | [Self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms)는 같은 사용자·workspace의 로컬 OAuth, 보호된 credential 전달, VM 고유 host ID 보존과 VM의 refresh 소유를 설명한다. 이전한 session의 host별 usage attribution/revocation은 아직 제공되지 않는다. 이 절차를 설치자 구독의 다중 회원 공유로 확대하지 않는다. |
| SIWC inference | [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)는 공개 `api.openai.com/v1/models`와 `/responses`, account별 모델 목록, `store:false`, `stream:true`, terminal `response.completed`를 요구한다. [Preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)는 HTTP `previous_response_id`, `max_output_tokens`와 여러 hosted tool을 지원하지 않는다. ChatGPT `backend-api`, Codex 로그인 파일 추출, scope 위장은 대안이 아니다. |
| 자체 Claude 제품/SDK | [공식 legal/compliance](https://code.claude.com/docs/en/legal-and-compliance)의 Authentication and credential use는 자체 제품/서비스와 Agent SDK에 Console API key 또는 지원 cloud를 안내한다. consumer Claude OAuth를 Rust HTTP 모델 adapter로 전달하는 기능은 만들지 않는다. |
| 변경 없는 Claude Code hosting | 같은 문서의 Can customers offer Claude Code in their products?는 별도 합의가 없으면 Commercial Terms 수락, 게시된 binary의 무변경 실행, 내장 인증 방식 유지, 각 최종 사용자의 자체 인증·직접 과금 등을 요구한다. “Customers may not pay for, resell, or intermediate Claude usage on their end users’ behalf.” 설치자의 Claude 구독을 모든 회원의 공용 사용량으로 중계하는 경로는 제공하지 않는다. |

공식 문서는 해당 설치·계정·모델의 사용 자격이나 계약 수락 증명이 아니다. 이 작업은 인증·결제·실제 provider 추론을 수행하지 않았다.

## 코드 대조

- 기존 [SIWC 프로그램](../../crates/moosuri-agent/src/subscriptions.rs)은 개인 `SubscriptionRef`와 `SubscriptionAuthority`를 먼저 확인하고, [Store](../../crates/moosuri-store/src/subscriptions.rs)가 owner별 봉인·refresh lease/CAS를 소유한다. [보호된 VM 전달](../../docs/chatgpt-authentication.md)은 이 개인 프로그램의 별도 경로다. 현재 Kiri member router는 이를 구독 설정/추론 경로로 연결하지 않았다. 설치 broker가 개인 DB를 대신 열거나 봉인된 자료를 공용 key 파일로 바꾸지 않는다.
- 기존 [Claude Profile](../../crates/moosuri-connectors/src/claude_code/sandbox.rs)의 `Profile::from_json`은 Linux non-root를 요구하고 euid 0을 거절한다. rootless engine, exact binary/image/profile, private auth/scratch root 및 accepted isolation/hosting 조건을 계속 검사한다. [공식 실행 계약](../../docs/claude-execution.md)은 그대로 적용된다.
- 반면 [Kiri 운영 안내](README.md)의 member text profile은 `broker_uid:0`을 고정한다. [broker](../../apps/moosuri/src/runtime_model.rs)는 registry의 owner-private 권한과 member별 socket chown을 요구한다. [provisioner](../../apps/moosuri/src/kiri_provision.rs)는 root 제어 경로와 서로 다른 member UID를 사용한다. 따라서 root broker에서 `Profile::stream_text`를 직접 호출하는 안은 실제 배포에서 성립하지 않는다.
- broker를 임의로 rootless로 바꾸거나 registry/키 권한을 넓히고 chown capability를 주는 안, Kiri guest에 rootful/rootless engine 소켓을 마운트하는 안은 이 차이를 해결하지 않는다. `synthetic_from_json`으로만 통과하는 경로도 기능으로 제공하지 않는다.

## 필요한 다음 독립 구현

구현 가능한 후보는 **root broker → 회원별 UID로 실행되는 별도 rootless executor**다. 아직 구현되거나 수락된 bridge는 아니다.

1. root broker만 registry·epoch·기존 단일 호출 원장을 소유하고 dispatch를 먼저 기록한다. rootless executor에는 registry, 다른 회원 볼륨, 설치 API credential을 전달하지 않는다.
2. member와 분리된 host IPC에서 커널 peer UID를 양방향 검사한다. root broker는 executor의 고정 UID·전체 owner/volume/generation·profile/root/environment/revision을 고정한다. executor는 root broker와 자기 소유자만 허용한다. 회원 요청이 URL, HOME, environment, session, engine 경로를 선택할 수 없게 한다.
3. 각 최종 사용자의 기존 공식 로그인·정확한 모델 검증과 현재 소유권에서 association을 만든다. 서로 다른 회원이 같은 인증 HOME을 공유하거나 config 변경으로 과거 환경을 재사용할 수 없게 한다. executor는 기존 `Profile`의 fresh session, `client_composed`, empty tools, no persistence, cleanup ACK와 retired-environment 검사를 재사용한다.
4. 취소·정지·resume epoch 변경·연결 해제·executor 재시작·비정상 suffix/EOF를 동일 호출 영수증으로 정리한다. 원격 완료/비용이 불명확하면 unknown이며 새 ID로 자동 재추론하지 않는다. 소비자 구독의 실제 결제·남은 allowance나 provider hard output-token cap을 로컬 예산과 혼동하지 않는다.
5. 실제 Linux에서 UID A/B의 교차 접근, root↔rootless 인증, host/guest mount, parent loss·descendant cleanup, retired HOME, quota 및 API regression을 먼저 검증한다. 이후에도 공식 binary·계정 로그인·실제 모델 수락은 별도다. SIWC의 회원별 경로를 추가하려면 그 배포 유형의 공식 허용 범위와 개인 봉인/host/refresh 소유부터 별도로 연결한다.

현재의 부분 대안은 기존 API/local broker를 사용하는 것이다. 기존 단일 사용자 SIWC/공식 Claude 프로그램은 각자의 문서상 승인된 환경에서 계속 사용할 수 있지만, 그것을 이 Kiri 구독 bridge의 구현 완료라고 설명하지 않는다.
