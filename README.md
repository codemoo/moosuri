# moosuri

A self-hosted assistant for individuals, families and friends.

**Status: early development. The Rust/React app supports isolated accounts, durable chat, private sources and memories, editable documents and goals, public HTTP research, isolated Chromium workspaces with human takeover and same-page continuation, and personal Gmail with explicit approval before every send. Source memory extraction is opt-in; fixed local reminder schedules and an inbox are available.**
The existing private deployment remains the behavioral reference; its source history,
accounts, data, infrastructure settings and experimental runtimes are not copied here.

- [홈 화면에 설치하기](docs/pwa.md): 공개 화면 오프라인 지원과 업데이트
- [현재 상태와 다음 작업](docs/status.md)
- [재작성 계획](docs/plan.md): 목표·순서·완료 조건
- [설계](docs/architecture.md): Rust 경계·실행·저장·복구
- [요구사항](docs/requirements.md): 사용자 지시 29개와 검증 추적
- [설치·관리자](docs/onboarding.md): 초기 설정·초대·대시보드·접속 방식
- [계정 관리](docs/account-management.md): 정지·재개·역할·임시 암호 재발급·모든 기기 로그아웃
- [사용자 격리](docs/accounts-isolation.md): 각자의 세션·실행 환경·사용량
- [모델 연결](docs/providers.md): OpenAI·Anthropic·OpenRouter·로컬 LLM
- [볼트 보안](docs/vault-security.md): 사용자 키·관리자 경계·복원
- [평가·테스트 계획](docs/testing.md): 메모리·대화·미션·스킬 검증과 기존 테스트 선별
- [구체적 검증 예제](docs/test-examples.md)와 [시나리오 표](audit/evaluation-scenarios.tsv)
- [공동 개발](CONTRIBUTING.md)과 [OpenSpec](openspec/README.md)
- [커뮤니티 스킬](docs/community-skills.md): GitHub 공유와 기존 제안 카드
- [업무 스킬](docs/task-skills.md): 자료 브리핑·회의 준비·근거 비교와 정확한 계산
- [외부 업무 스킬](docs/external-task-recipes.md): 웹 조사·메일 브리핑·승인 대기 답장 초안
- [개인 비서 업무 사례](docs/research/personal-agent-tasks-2026-11-13.md): Muse 중심의 공개 근거와 구현 범위
- [웹·이메일 실행 계약](docs/external-work.md): 브라우저 격리·개인 연결·발송 전 최종 승인
- [개인 Gmail](docs/mail.md)·[웹 조사](docs/web-research.md)·[격리 브라우저](docs/browser-chat.md): 연결 설정·사용법·지원 범위
- [개인 작업 브라우저](docs/browser-workspaces.md): 같은 로그인 세션·화면 보기·직접 조작·대화 이어가기
- [뮤즈·피클 브라우저 조사](docs/research/browser-workspaces-2026-11-13.md): 공식 근거와 채택 범위
- [credential 자동 사용](docs/credential-use.md): 볼트 연결·재사용·잠금/철회 처리
- [데이터 수집 연결](docs/data-connections.md): 공식 Gmail·Slack과 커스텀 수집, 출처·동기화 추적
- [기능별 이전표](docs/features.md): 유지할 동작과 검증 기준
- [이전 순서](docs/migration.md): 코드·데이터·운영 전환
- [전수 조사와 제외 목록](docs/audit.md): 파일별 판정과 근거
- [공개 배포](docs/public-release.md): 설치·기여·공개 범위
- [검토한 공식 자료](docs/references.md)

This repository contains the reviewed plan and source audit, a Rust account/API/CLI foundation, a React interface, shared model settings, durable chat with private knowledge retrieval, editable memories and a protocol/transport library, pinned OpenSpec tooling and contributor scaffolding. The public development repository is [codemoo/moosuri](https://github.com/codemoo/moosuri). It does not contain a model key or copied production data.

GitHub Actions/CI are disabled during initial development. Contributors run relevant checks locally and commit/push each coherent work unit. Personal coding-agent instructions are ignored; shared contracts live in OpenSpec and CONTRIBUTING.

Build and run the development preview with [the quickstart](docs/development.md). It includes hidden-password initialization, account login, local invitations and separate conversations. Administrators can configure shared connections, discover and verify models, and users can select model/effort in [model settings](docs/provider-settings.md). [Text chat](docs/chat.md) supports streaming, cancellation, retry and reconnect. [Private knowledge](docs/knowledge.md) supports text sources, exact evidence, editable memories and scoped deletion. [The knowledge agent](docs/knowledge-agent.md) connects verified tools and revocable source references to chat. [Conversational memory](docs/conversational-memory.md) adds explicit remember/correct/forget requests. [Source memory extraction](docs/automatic-memory.md) processes opted-in collections with literal evidence, bounded background work and exact-quote forget suppression. [Bundled task skills](docs/task-skills.md) add briefing, meeting preparation and comparisons using saved evidence and exact decimal arithmetic. Three [external task recipes](docs/external-task-recipes.md) add public research, inbox briefings and response drafts with mandatory final send approval. [Public HTTP research](docs/web-research.md) reads pages and searches with an optional Brave key. Optional [Linux Chromium capsules](docs/browser-chat.md) read rendered public pages with a separately verified deployment. [Browser workspaces](docs/browser-workspaces.md) keep one isolated page/profile per conversation, let the user watch and take control for login, then continue in the same conversation from that page. Automatic authenticated form actions and long-term profile recovery remain pending. [Personal Gmail](docs/mail.md) reads mail and prepares local drafts; sending requires the user's final approval of the complete preview. Vault, calendar writes and scheduling remain unavailable.

[Live ChatGPT evaluation](docs/reviews/2026-11-13-live-chatgpt-quality.md) used the existing official login with `gpt-6.1-terra` / `high`. The earlier repeated development corpus matched36/36 live intent labels; end-to-end scenarios passed7/10. [Memory-target corrections](docs/reviews/2026-11-13-memory-target-resolution.md) subsequently passed synthetic checks, but the new12-case live holdout remains unattempted after connection-probe failures. These are limited development results, not a shipped subscription connection or an overall assistant quality score.

Validate planning metadata with Rust 1.89 or newer:

```sh
cargo run --locked -p xtask -- verify-plan
```

The application stack is **Rust backend/agent/CLI + React/TypeScript web + SQLite**.
All user, admin, vault and PWA screens use React. Shared model connections serve
separate user environments. Linux is the deployment target; Tailscale-private and
ordinary HTTPS self-hosting are supported design paths. PostgreSQL is future work.
Account-owned Gmail/Slack collection and a generic custom HTTP ingest protocol are implemented.
Each user opts into a bounded source scope and can inspect sync history and provenance
in `/settings/sources`. Gmail uses the operator's OAuth app, configured once in the
[Google setup wizard](docs/mail.md) at `/admin/google`; every member grants their own
Google account access. Slack accepts a
personal user token. Synthetic Linux acceptance does not replace live-account setup.
Kakao collection is privately operated and excluded from public packages; its data
can use the custom protocol. [Scope and implementation tracking](docs/data-connections.md).
This does not claim that every transitive dependency is implemented in Rust.
The exact boundary and build gates are in [architecture](docs/architecture.md).

Licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option.

개인 기록의 범위와 한계: [문서·목표·요청](docs/artifacts.md).

Fixed local [reminders and inbox](docs/schedules.md) are available at `/app/schedules`;
recurring agent missions and Web Push remain in development.
