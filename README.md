# moosuri

A self-hosted assistant for individuals, families and friends.

**Status: early development. The Rust/React account and model setup foundation runs; chat/agent execution is not connected yet.**
The existing private deployment remains the behavioral reference; its source history,
accounts, data, infrastructure settings and experimental runtimes are not copied here.

- [현재 상태와 다음 작업](docs/status.md)
- [재작성 계획](docs/plan.md): 목표·순서·완료 조건
- [설계](docs/architecture.md): Rust 경계·실행·저장·복구
- [요구사항](docs/requirements.md): 사용자 지시 28개와 검증 추적
- [설치·관리자](docs/onboarding.md): 초기 설정·초대·대시보드·접속 방식
- [사용자 격리](docs/accounts-isolation.md): 각자의 세션·실행 환경·사용량
- [모델 연결](docs/providers.md): OpenAI·Anthropic·OpenRouter·로컬 LLM
- [볼트 보안](docs/vault-security.md): 사용자 키·관리자 경계·복원
- [평가·테스트 계획](docs/testing.md): 메모리·대화·미션·스킬 검증과 기존 테스트 선별
- [구체적 검증 예제](docs/test-examples.md)와 [시나리오 표](audit/evaluation-scenarios.tsv)
- [공동 개발](CONTRIBUTING.md)과 [OpenSpec](openspec/README.md)
- [커뮤니티 스킬](docs/community-skills.md): GitHub 공유와 기존 제안 카드
- [credential 자동 사용](docs/credential-use.md): 볼트 연결·재사용·잠금/철회 처리
- [기능별 이전표](docs/features.md): 유지할 동작과 검증 기준
- [이전 순서](docs/migration.md): 코드·데이터·운영 전환
- [전수 조사와 제외 목록](docs/audit.md): 파일별 판정과 근거
- [공개 배포](docs/public-release.md): 설치·기여·공개 범위
- [검토한 공식 자료](docs/references.md)

This repository contains the reviewed plan and source audit, a Rust account/API/CLI foundation, a React interface, shared model settings and a protocol/transport library, pinned OpenSpec tooling and contributor scaffolding. The public development repository is [codemoo/moosuri](https://github.com/codemoo/moosuri). It does not contain a model key or copied production data.

GitHub Actions/CI are disabled during initial development. Contributors run relevant checks locally and commit/push each coherent work unit. Personal coding-agent instructions are ignored; shared contracts live in OpenSpec and CONTRIBUTING.

Build and run the account foundation with [the development quickstart](docs/development.md). It includes hidden-password initialization, account login, local invitations and separate empty conversation lists. Administrators can configure shared connections, discover and verify models, and users can select model/effort in [model settings](docs/provider-settings.md). Model chat, vault and external work are still unavailable.

Validate planning metadata with Rust 1.89 or newer:

```sh
cargo run --locked -p xtask -- verify-plan
```

The application stack is **Rust backend/agent/CLI + React/TypeScript web + SQLite**.
All user, admin, vault and PWA screens use React. Shared model connections serve
separate user environments. Linux is the deployment target; Tailscale-private and
ordinary HTTPS self-hosting are supported design paths. PostgreSQL is future work.
This does not claim that every transitive dependency is implemented in Rust.
The exact boundary and build gates are in [architecture](docs/architecture.md).

Licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.
