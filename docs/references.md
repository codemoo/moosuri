# 설계 근거와 확인 범위

확인일2026-11-12. 공식 문서·maintainer 문서/소스를 우선했다. 공급자 문서는 계획의 근거이며 해당 계정/모델의 실제 연결 성공을 뜻하지 않는다. dependency 버전은 P0에서 pin하고 출시 직전 API/인증/정책을 다시 확인한다. 경쟁 제품의 공개 UX 관찰을 내부 아키텍처를 안다는 주장으로 바꾸지 않는다.

## 이번 계획에서 확인한 기술 자료

| 자료 | 설계에 반영한 판단 |
| --- | --- |
| [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), [2024 resolver](https://doc.rust-lang.org/stable/edition-guide/rust-2024/cargo-resolver.html) | 단일 workspace/lock, MSRV·feature 경계 |
| [Axum](https://docs.rs/axum/latest/axum/), [Reqwest](https://docs.rs/reqwest/latest/reqwest/), [Rustls](https://docs.rs/rustls/latest/rustls/) | Rust HTTP/API 후보, client pool 수명, TLS native 의존성 별도 검토 |
| [Rig durable runs](https://www.rig.rs/docs/concepts/runs) | serializable sans-IO AgentRun은 후보. 앱이 journal·receipt·권한을 소유하고 pending IO 재개를 시험 |
| [SQLite WAL](https://www.sqlite.org/wal.html), [transactions](https://www.sqlite.org/lang_transaction.html) | 한 호스트 local FS, 단일 writer·짧은 transaction·busy 처리 |
| [SQLite online backup](https://www.sqlite.org/backup.html), [FTS5](https://www.sqlite.org/fts5.html) | 활성 DB 단독 파일 복사 금지, 검색 동등성 별도 평가 |
| [RustCrypto Argon2](https://docs.rs/argon2/latest/argon2/) | KDF 구현 후보. 실제 vault primitive/format은 기존 합성 벡터로 고정 |
| [OpenAI Sign in with ChatGPT](https://developers.openai.com/siwc), [OSS token sharing](https://developers.openai.com/siwc/token-sharing-open-source.md) | OSS plan usage 지원 경로, 개인/workspace 동의. 일반 API key와 구분 |
| [SIWC self-hosted VMs](https://developers.openai.com/siwc/token-sharing-open-source/self-hosted-vms.md), [preview limits](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations.md) | host ID/refresh 소유·원격 설치 흐름, stateless 제한·지원 field/tool/media 구별 |
| [ChatGPT auth](https://learn.chatgpt.com/docs/auth), [app-server auth](https://learn.chatgpt.com/docs/app-server#auth-endpoints) | OAuth/app-server 용도와 hosted/상용 경로를 혼동하지 않음 |
| [Claude legal/compliance](https://code.claude.com/docs/en/legal-and-compliance), [authentication](https://code.claude.com/docs/en/authentication), [env vars](https://code.claude.com/docs/en/env-vars) | 자체 하네스 API와 조건부 공식 CLI hosting 구별; consumer token 중계 금지·개인 인증 |
| [OpenRouter quickstart](https://openrouter.ai/docs/quickstart) | API·catalog 연결 후보, 실제 route/capability·privacy/fallback 설정 검증 |
| [Ollama compatibility](https://docs.ollama.com/api/openai-compatibility) | subset adapter, 모델과 local/cloud 경로별 검증 |
| [LM Studio compatibility](https://lmstudio.ai/docs/developer/openai-compat) | 로컬 API preset, model별 tool/stream 검증 |
| [vLLM compatible server](https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/) | 현재 문서 위치 확인, model/template별 API capability 검증 |
| [GWS README pinned](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/README.md), [changelog](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/CHANGELOG.md) | native Rust CLI, 공식 지원 Google 제품 아님. 구 account 명령 제거 확인 |
| [GWS auth](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/crates/google-workspace-cli/src/auth.rs), [credential store](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/crates/google-workspace-cli/src/credential_store.rs) | CONFIG_DIR/credential/cache·keyring·fresh process isolation |
| [GWS send helper](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/crates/google-workspace-cli/src/helpers/gmail/send.rs), [commands](https://github.com/googleworkspace/cli/blob/cd3697e07eb520056dcf142d0c7439bbb9ba32e9/crates/google-workspace-cli/src/commands.rs) | argv body와 private upload 경로 구별, exact release binary fixture 검증 필요 |
| [Google web OAuth](https://developers.google.com/identity/protocols/oauth2/web-server), [token expiration](https://developers.google.com/identity/protocols/oauth2#expiration) | per-user state/callback·testing token expiry·reconsent |
| [Gmail scopes](https://developers.google.com/workspace/gmail/api/auth/scopes), [send guide](https://developers.google.com/workspace/gmail/api/guides/sending), [messages.send](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send) | 최소 send scope, MIME, sent receipt와 실제 delivery 구별 |
| [Tailscale Serve](https://tailscale.com/docs/features/tailscale-serve), [Funnel](https://tailscale.com/docs/features/tailscale-funnel), [HTTPS](https://tailscale.com/docs/how-to/set-up-https-certificates) | private/public 구별, trusted headers, app account 별도·origin/TLS |
| [Caddy HTTPS](https://caddyserver.com/docs/automatic-https) | 일반 self-host 예제, TLS·DNS·proxy 운영 조건 |
| [Anthropic Building effective agents](https://www.anthropic.com/research/building-effective-agents) | 단순한 bounded 구성부터 시작. 문서 자체가 이후 제품 발전을 알리므로 최신 API 명세로 사용하지 않음 |
| [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence) | thread checkpoint와 장기 store를 분리하는 개념 유지; Python runtime 채택과는 별개 |

GWS source snapshot은 main commit과 release artifact가 동일하다고 가정하지 않는다. inspect한 version 문자열0.22.5만으로 동작을 확정하지 않고 사용할 binary/checksum으로 auth/upload fixture를 실행한다. 이 계획에서 계정 로그인·메일 발송·유료 model probe·production 변경은 하지 않았다.

## 기존 조사에서 이어받는 아키텍처 근거

아래는 기존 제품의 설계/시험 기록에서 출처를 추적한 참고 자료다. 이번에 모두 새 버전으로 재검증한 것처럼 표시하지 않는다. P0/관련 구현 시 원문과 버전을 다시 확인한다.

| 자료 | 가져올 원칙·한계 |
| --- | --- |
| [Hermes skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills), [memory](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory) | 성공 절차의 재사용·지속 기억 아이디어. 외부 runtime 통째 도입/무제한 자동 code 생성은 제외 |
| [OpenClaw architecture](https://docs.openclaw.ai/concepts/architecture), [cron jobs](https://docs.openclaw.ai/automation/cron-jobs) | 세션·gateway·예약/알림 경계 참고. moosuri receipt/삭제 계약은 자체 검증 |
| [Letta memory blocks](https://docs.letta.com/v1-sdk/memory/memory-blocks) | 편집 가능한 지속 기억, 전체 자료를 매번 prompt에 넣지 않음 |
| [Anthropic context engineering](https://www.anthropic.com/research/effective-context-engineering-for-ai-agents), [writing tools](https://www.anthropic.com/research/writing-tools-for-agents) | 필요한 문맥·schema만 읽기, 명확한 tool input/output·평가 |
| [Anthropic long-running harnesses](https://www.anthropic.com/research/effective-harnesses-for-long-running-agents), [agent evals](https://www.anthropic.com/research/demystifying-evals-for-ai-agents) | 원래 조건·진척·검증·실패 분모를 지속 보존 |
| [Manus context engineering](https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus) | 모델 문맥/도구 결과 관리 참고; 자체 운영 주장과 일반 보장 구별 |

Hermes/OpenClaw의 과거 prototype은 기존 복구/미완료 checkpoint gate에서 채택되지 않았고 필요한 동작만 기존 제품에 반영했다. 그 결과를 새 Rust 구현이 검증됐다는 뜻으로 쓰지 않는다. 뮤즈/피클/그록봇은 제품 UX 기준이며 내부 하네스의 비공개 구현은 알 수 없다. 비공개 메일/PDF 원문은 공개 자료 목록이나 fixtures에 넣지 않는다.

## 새 평가·공동 개발·스킬 공유 조사

다음 자료는 이번 추가 계획에서 공식 프로젝트 문서/소스를 확인했다. 외부 benchmark의 점수를 moosuri 성적으로 인용하지 않으며 문제/코드/fixture의 그대로 반입을 결정한 것도 아니다.

| 자료 | 반영한 판단·한계 |
| --- | --- |
| [Anthropic agent evaluations](https://www.anthropic.com/research/demystifying-evals-for-ai-agents) | agent 주장과 실제 outcome 구분, code/model/human grader 분업, 반복 실패와 trace 분석. 상태/권한 oracle은 모델 판정에 맡기지 않음 |
| [LongMemEval](https://github.com/xiaowu0162/LongMemEval/blob/main/README.md), [LongMemEval V2](https://github.com/xiaowu0162/LongMemEval-V2/blob/main/README.md) | 장기·다중 세션·갱신·시간·모름 및 static/dynamic workflow 기억. retrieval과 최종 정확도/지연 분리, gold를 memory backend에 제공하지 않음 |
| [τ-bench 현재 maintained repository](https://github.com/sierra-research/tau2-bench/blob/main/README.md), [구 tau-bench](https://github.com/sierra-research/tau-bench/blob/main/README.md) | 현재 README는 τ-bench2.0과 한국어 포함 multilingual을 안내. task reward와 style judge 구별·frozen tasks·버전간 점수 직접 비교 금지. 구 repo는 outdated 안내 |
| [OpenSpec README](https://github.com/Fission-AI/OpenSpec/blob/main/README.md), [package](https://github.com/Fission-AI/OpenSpec/blob/main/package.json), [schema](https://github.com/Fission-AI/OpenSpec/blob/main/schemas/spec-driven/schema.yaml) | 2.0.0을 repo-local로 pin. established specs와 pending changes 구별·관측 가능한 SHALL/MUST 시나리오·구조 검증은 구현 검증 아님 |
| [OpenSpec supported tools](https://github.com/Fission-AI/OpenSpec/blob/main/docs/supported-tools.md), [FAQ](https://github.com/Fission-AI/OpenSpec/blob/main/docs/faq.md), [propose workflow](https://github.com/Fission-AI/OpenSpec/blob/main/src/core/templates/workflows/propose.ts) | 최신 openspec-* skill 이름, 작은 변경은 ceremony 생략. propose는 계획 전용이며 이미 요청된 구현은 direct CLI/normal task로 이어감 |
| [Agent Skills specification](https://agentskills.io/specification), [source](https://github.com/agentskills/agentskills/blob/main/docs/specification.mdx) | SKILL.md·필요할 때만 references 로딩. 파일 표준과 runtime/권한 호환은 별개 |
| [Vercel skills README](https://github.com/vercel-labs/skills/blob/main/README.md), [commands](https://github.com/vercel-labs/skills/blob/main/docs/commands.md), [lock implementation](https://github.com/vercel-labs/skills/blob/main/src/local-lock.ts) | Git source/path 설치·exact revision/content·local edits 보호 참고. Node installer를 제품 runtime으로 반입하지 않음 |
| [ClawHub](https://docs.openclaw.ai/clawhub), [CLI](https://github.com/openclaw/clawhub/blob/main/docs/cli.md), [security audits](https://github.com/openclaw/clawhub/blob/main/docs/security-audits.md) | 탐색/설치/update/rollback과 출처 검사 참고. marketplace 서버·telemetry·runtime 전체 도입 없이 Git/static catalog/기존 카드로 단순화 |

이번 설치는 scratch의 고정 OpenSpec2.0.0만 사용하며 전역1.5.0은 변경하지 않았다. package-lock으로 transitive dependencies도 고정한다. GitHub CI action ref는 공식 repository의 `v4.2.2` checkout(`11bd719…`)과 `v4.4.0` setup-node(`49933ea…`) tag object에서 full commit을 확인해 workflow에 기록했다. CI 자체는 아직 GitHub에서 실행되지 않았다.

CredentialBroker는 위 제품들의 비공개 구현을 복제했다고 주장하지 않는다. 사용자 키 vault·typed adapter·원래 요청/receipt 계약을 결합한 새 설계이며 pairing/transport/기기 인증 protocol은 P0/P8에서 검증된 라이브러리를 선정하고 독립 검토한다.

## 백업 복구 키의 표준 형식 확인

2026-11-12 [age README](https://github.com/FiloSottile/age/blob/main/README.md), [Rust rage README](https://github.com/str4d/rage/blob/main/README.md), [workspace manifest](https://github.com/str4d/rage/blob/main/Cargo.toml)을 확인했다. 공개 recipient로 암호화하고 private identity로 해독하는 분리와 Rust 구현·`age` library 사용 안내를 복구 키 설계의 근거로 삼는다. 비밀 키 예제나 실제 key는 저장소로 가져오지 않는다.

확인한 upstream main은 `age`0.12.1/MSRV1.85를 명시하지만 이것을 실제 채택/설치·release 가용성·보안 감사 완료라고 표현하지 않는다. P0에서 사용할 release/hash·feature·상호운용·security advisory를 검증한 뒤 lock한다. 기본 제품에 별도 Go runtime/age CLI를 추가하지 않는다.
