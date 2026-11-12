# 모델·구독·Google 연결

공용 API/local 연결과 개인 구독 연결을 분리한다. 설치 화면은 연결 방식→인증→실제 model 목록→기능 확인→기본 model/effort 순서다. 관리자가 허용한 범위에서 사용자별 기본값을 고를 수 있다. 모든 행은 구현 대상이며 현재 연결 완료를 뜻하지 않는다. 공식 자료는 [references](references.md)에 있다.

현재 Rust text protocol/HTTP 구현 범위는 [model transport](model-transport.md)에 있다. 합성 fixture/loopback 검증을 실제 계정 연결·capability probe·웹 대화 완료로 표시하지 않는다.

## 지원 행렬

| 연결 | 인증·공유 | 구현·지원 범위 |
| --- | --- | --- |
| OpenAI API | 설치 공용 또는 개인 API key | Rust Responses adapter, 실제 모델별 tools/stream/effort/media/usage 지원 검사 |
| ChatGPT plan | 공식 Sign in with ChatGPT의 사용자별 동의·workspace | 자체 OSS 앱용 공식 흐름. plan usage 허용과 모델 가용성을 검사; 한 사용자의 구독을 전체 가족 pool로 취급하지 않음 |
| Anthropic API | 공용 또는 개인 API key | Rust Messages adapter. 구독과 별도 과금이며 tool result/stop reason/token 의미를 자체 변환 |
| Claude 구독 | 각 사용자 본인의 공식 Claude Code 로그인 | 변경하지 않은 CLI를 격리 실행하는 선택 통합. 자체 Rust ModelPort에 Claude.ai OAuth token을 전달하는 방식은 제공하지 않음. durable/tool/격리 검증 전 unavailable |
| OpenRouter | 공용 또는 개인 API key | Responses 또는 검증한 Chat Completions adapter, model catalog·실제 route·privacy/fallback·usage를 기록 |
| Ollama | 설치자가 등록한 local/LAN endpoint, 필요시 proxy auth | OpenAI-compatible subset 또는 native adapter; 설치된 model별 tool/stream/image 지원 확인 |
| LM Studio | 등록한 local/LAN endpoint·server auth 설정 | OpenAI-compatible Responses/Chat endpoint, model별 tool/chat template와 capability 확인 |
| vLLM | 등록한 자체 서버·API key/네트워크 정책 | 검증한 OpenAI-compatible endpoint·chat template/tool parser·model별 지원 검사 |
| Generic compatible | admin이 등록한 endpoint와 credential | 선택적 호환 adapter. API 모양이 같아도 capability probe 없이는 전체 지원 표시 금지 |

처음에는 실제 연결 가능한 모델 목록으로 선택한다. 기존 private 사용자의 GPT-6.1 Terra/high 선호는 importer가 보존하되 해당 계정에 실제 제공되는 정확한 ID인지 확인한다. 공개 설치에 존재하지 않는 모델 ID나 effort를 하드코딩하지 않는다. effort 미지원은 숨기거나 unavailable로 표시하고 다른 설정으로 몰래 대체하지 않는다.

## ModelPort와 설정 수명

중립 DTO는 message role/content/tool call/result ID·finish/error·usage·stream event를 갖는다. `ProviderCapabilities`는 model/operation별 stream, tools/parallel tools, JSON schema, image/audio, context limit, output bound, reasoning/effort, hosted search, usage·cancel/reconcile 의미를 기록한다. capability에는 검증한 provider/model/version/date가 붙는다. 모델이 도구를 지원하지 않으면 대화 모드만 제공하고 본문을 tool call처럼 해석하지 않는다.

공용 provider profile은 endpoint ID·auth mode·secret reference·model allowlist·default·fallback/data-routing 정책·budget/version을 가진다. user binding은 접근 권한과 개인 auth/override를 가진다. 키는 Rust credential endpoint로만 받고 UI에는 configured/fingerprint/status만 돌려준다. 모델에게 키·환경변수·raw provider response debug를 보여주지 않는다. 개인 OAuth refresh는 binding/version별 single-flight로 처리한다.

run은 provider version/model/effort/capability/budget policy를 고정한다. 기본값 변경은 새 run부터 적용한다. 진행 run은 기존 version이 유효할 때만 이어가며 폐기된 version을 사용할 수 없으면 checkpoint를 보존하고 pause한다. 즉시 revoke는 새 reservation/dispatch를 막고 기존 호출을 취소한다. 이미 보낸 호출은 원격 완료/요금이 남을 수 있어 receipt/사용량을 계속 정산하며 새 ID로 재호출하지 않는다.

자동 fallback은 기본 꺼짐이다. 활성화할 때 model·provider·지역/데이터 처리·비용 범위를 명시한다. OpenRouter 자체 route도 이 정책에 맞게 제한하며 실제 route/불명 여부를 기록한다. local-only 사용자의 요청을 클라우드로 보내지 않는다. 연결 장애는 해당 provider 큐에만 영향을 주고 다른 사용자의 작업을 막지 않는다.

## ChatGPT 공식 로그인

2026-11-12 확인한 공식 SIWC 문서는 오픈소스 앱의 선택적 ChatGPT plan usage를 지원한다. 사용자/workspace 동의와 앱 등록 조건을 준수하며 ChatGPT의 기존 대화 기록을 가져오는 기능으로 설명하지 않는다. PKCE/state/nonce·scope·logout/revoke/refresh 수명을 구현한다. host별 불투명 `ext_agent_host_id`를 지속 저장하며 다른 VM/laptop의 host ID를 복제하지 않는다.

원격 Linux 설치는 공식 self-hosted VM 흐름에 맞게 로컬 브라우저 인증·보호된 credential 전달·VM refresh 소유를 설계한다. hosted/유료 서비스는 그에 맞는 공식 통합 조건을 다시 확인한다. app-server의 과거 로그인 예제를 소비자 token 공유 API로 쓰지 않는다.

preview adapter는 `store:false`, `stream:true`, input array와 로컬 durable history를 사용한다. HTTP `previous_response_id`에 의존하지 않는다. `max_output_tokens` 등 미지원 필드, hosted file/tool search·code interpreter·computer use·hosted MCP·이미지 생성·Files/audio/video 경로를 별도 capability로 차단한다. 앱의 도구 발견은 자체 ToolRegistry이며 Responses hosted tool_search와 다르다. 사용량/시간/반복 상한은 지원되는 기능과 앱 admission에서 관리하고 provider가 지원하지 않는 hard output cap을 보장하지 않는다. 출시 시 최신 공식 제한과 실제 계정 access를 재검증한다.

## Claude 공식 실행 연결

Anthropic 문서는 자체 제품/Agent SDK에는 API key 또는 지원 cloud를 사용하도록 한다. Claude.ai OAuth/session cookie 추출·중계는 지원하지 않는다. 별도 조건을 지킨 변경 없는 공식 Claude Code hosting은 가능하며 각 사용자가 자기 계정으로 인증하고 직접 과금된다. CLI를 선택 설치할 때 외부 runtime/라이선스/버전·기능 차이를 표시한다. 기본 Rust 서버 실행에 필수로 넣지 않는다.

독립 user home·공식 auth UX·세션 mapping·OS sandbox를 유지한다. Moosuri가 subscription bearer token을 읽어 자체 HTTP adapter로 재사용하지 않는다. CLI가 직접 내는 임의 shell/외부 효과와 Moosuri receipt 계약을 자동 동등 취급하지 않는다. typed tool bridge·취소·미완료 checkpoint 재개·outcome_unknown 처리·리소스 격리를 검증해야 해당 업무를 활성화한다. 이 검증이 안 되면 API adapter를 기본으로 유지하고 공식 CLI 연결은 제한 상태로 남긴다.

## 로컬 모델과 네트워크

`localhost`는 Rust 서버가 실행되는 기기다. 사용자 PC의 Ollama/LM Studio에 연결하려면 LAN/Tailscale 주소 또는 container host gateway를 설정한다. 모델 설치 자체를 자동 수행하거나 모든 local endpoint에 cloud privacy 보장을 붙이지 않는다. Ollama cloud 등 원격 route와 로컬 실행을 구분한다.

admin만 scheme/host/port/base path·허용 IP/range·TLS/auth/proxy를 등록한다. model/일반 사용자 입력은 endpoint ID만 선택한다. 의도한 LAN/loopback은 허용하고 metadata/link-local·미등록 internal URL·userinfo·위험 scheme은 거절한다. DNS 검사 후 재해석되는 틈 없이 연결 대상을 고정하고 매 연결 검증한다. redirect는 기본 금지, 필요시 모든 hop과 credential 전달을 검증한다. HTTPS 검증을 끄는 우회는 없다. local HTTP는 명시한 제한 네트워크 설정이다.

health→models→text/stream→tool/result→취소/실패/usage probe를 합성 입력으로 실행한다. tools 미지원·서버 미실행·context 초과는 구체적인 상태로 표시한다. remote conversation/file/cache ID는 사용자·provider·local thread mapping을 통해서만 사용한다. 공유 inference의 private prompt/result cache가 사용자 데이터 조회 수단이 되지 않게 한다.

## Google 연결·gws·초대 발송

앱 로그인, 초대받은 이메일, 개인 Gmail/Calendar 계정, 시스템 초대 sender는 다른 정체성이다. `google_connections`는 user/binding/provider subject/scope/consent revision/purpose를 갖는다. admin sender는 `purpose=installation_invites`, 일반 사용자 connector는 개인 소유다. 하나를 해제해 다른 사람의 연결을 지우지 않는다.

React Google 연결은 Rust web OAuth authorization code + exact callback/state/user binding으로 만든다. remote 서버의 gws loopback login을 완성된 웹 OAuth처럼 설명하지 않는다. Gmail send-only 최소 scope로 초대 메일을 시작하며 read/compose는 해당 기능을 켤 때 추가한다. Google OAuth testing의 token 만료와 공개 앱 검증 조건은 설치 안내에 명시한다.

gws는 Google Workspace 조직의 Rust CLI지만 upstream은 공식 지원 Google 제품이 아니라고 명시한다. native checksummed artifact를 선택 설치하고 exact binary로 검사한다. 확인한 upstream에서는 `--account`, auth list/default 프로필 지침을 사용할 수 없다. binding마다 `GOOGLE_WORKSPACE_CLI_CONFIG_DIR`, credential file, cache, private OS home/cwd를 분리한다. inherited token/ADC·proxy·logging env를 비우고 `.env`/keyring/OnceLock 혼선을 fresh child process로 막는다. credential file만 바꾸고 global cache를 공유하지 않는다.

초대는 계정/temporary credential generation과 outbox/effect ID를 먼저 저장한 뒤 보낸다. URL에 비밀번호를 넣지 않는다. temp password를 메일에 넣는 선택에서는 encrypted short-lived outbox payload만 보관하고 전송 terminal 상태 또는 TTL에 제거한다. 암호화키는 SQLite와 별도의 private server secret이며 vault key와 무관하다. 본문/비밀을 argv/log에 넣지 않는다. gws `+send --body`와 dynamic `--json`은 문자열 인자라 stdin/@file을 지원한다고 가정하지 않는다. private MIME `--upload` 경로를 pinned artifact로 검증하거나 native Rust Gmail HTTP transport를 사용한다.

응답 유실은 같은 invitation/effect ID의 delivery_unknown이다. send-only scope로는 sent-message 조회가 불가능할 수 있으므로 무단 read scope 추가·자동 재발송을 하지 않는다. 재발급/재발송은 사용자가 실행하는 관리 동작이며 이전 credential을 폐기한다. provider 수신 영수증과 실제 이메일 도착/사용자 가입 완료는 다르다. [설치](onboarding.md)에 이 상태를 표시한다.

## 출시 전 provider 계약 시험

모든 adapter에서 stream split/tool-call ID·잘못된 JSON/role·429/5xx·중간 단절·취소·완료 뒤 중복 event·usage 누락·model/effort 불일치를 시험한다. 두 사용자의 동시 실행·refresh race·provider default 변경/revoke·remote ID 충돌·fair queue·중복 정산0을 실제 Rust 경로로 검증한다. local offline에는 외부 DNS/HTTP 요청0을 기록한다. 유료 live 평가와 실제 OAuth는 별도 시험 연결로 진행하며 합성 fixture 통과를 실계정 검증으로 부르지 않는다.

## 개인 볼트와 credential 선택

기존 server-managed API/OAuth 연결과 [user-key vault broker](credential-use.md)는 구분한다. 연결된 개인 binding은 같은 사용자·서비스·허용 operation/resource/recipient 범위에서 자동 재사용한다. 첫 계정 선택·실제 모호함·인증 만료 때만 연결 화면을 연다. 모델에게 비밀번호/API key/TOTP를 읽어오게 하지 않는다. 공유 LLM profile의 설치 credential이 가족의 private vault 접근 권한을 의미하지 않는다. broker가 unavailable이면 대기하며 서버 보관 token으로 조용히 우회하지 않는다.
