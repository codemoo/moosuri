# 공개 배포·설치 계획

공개 개발 저장소는 [codemoo/moosuri](https://github.com/codemoo/moosuri)다. 사용자가 생성·공개·작업 단위별 commit/push를 승인했다. P0 공개 허용 목록·라이선스·비밀 검사를 거쳐 새 이력을 올리며 첫 안정판은 P10이다. 운영 전환은 아직 하지 않았다. 새 공개 저장소 이름은 `moosuri`이며 새 Git 이력으로 시작한다. 기존 `.git`·개인 문서·운영 trace를 정리하지 않은 채 옮기지 않는다.

## 공개할 것과 제외할 것

공개 대상은 Rust source/lock, React/TypeScript source/lock, SQL baseline, 공개 가능한 합성 fixtures·tests, 일반 배포 예제, 사용자/관리자/기여 문서, SBOM·license notices, source-relative audit metadata다. 실제 개인 원문·메일/PDF·source DB·vault/credential·사용자 기억·실제 모델 trace·개인 domain/IP/UID·private 설치 설정은 반입하지 않는다. 옛 source snapshot·실험용 vendor runtime·generated build/cache는 넣지 않는다.

비공개 뮤즈 메일/PDF에서 유래한50개 절차의 문장을 공개 catalog로 복사하지 않는다. 동작 요구를 공개 문서/자체 설계로 다시 쓴다. 뮤즈를 주 UX 참고, 피클/그록봇을 보조로 삼되 상표·화면 카피·제3자 일러스트를 그대로 배포하지 않는다. 귀여운 픽셀 시종 방향의 자체 moosuri 아이콘과 합성 screenshots를 준비한다. 기존 자산 권리/생성 이력이 불명확하면 새로 제작한다.

새로 작성한 공개 코드·문서에는 MIT OR Apache-2.0을 적용한다. 최초 공개에는 새 Rust 계획 검증기·자체 설계·상대 경로/해시 조사표만 포함하며 기존 private source·제3자 runtime/자산은 복사하지 않는다. 원본의 불명확한 라이선스를 이 공개 라이선스로 덮어쓰지 않는다. dependency와 코드 인용의 라이선스는 별도 검사한다. source inventory는 반입 허용 목록이 아니라 재작성 근거 목록이다. 로컬 AGENTS.md·CLAUDE.md와 개인 editor 지침은 .gitignore로 제외한다.

공식 mail/Slack 수집 adapter와 generic custom HTTP protocol은 [수집 계획](data-connections.md)에
따라 별도로 구현·검증한다. **카톡 수집 코드·전용 도구·설치 스크립트·DB driver는 공개 배포에
넣지 않는다.** 개인 외부 producer는 사용자가 운영하며, generic payload를 쓰는 데 개인
수집기 source나 credential을 공개할 필요가 없다. 분류와 실제 출시/준비 상태는 구별한다.

## 쉬운 설치의 완료 조건

Linux x86_64/arm64에 signed/checksummed binary와 내장 React assets를 배포한다. 설치 경로는 standalone binary를 우선하고 OCI/Compose는 같은 private volume 설정의 선택지다. 기본 실행에 Python·Node·PostgreSQL·Redis·외부 vector DB가 없어야 한다. 현재 개발판의 `moosuri init/serve/doctor/export-types`는 실행 가능하며 [개발 안내](development.md)에 사용법/한계를 둔다. `backup/restore/update/uninstall`은 미구현이다. signed release/installer가 완성됐다고 안내하지 않는다.

init은 admin 하나·password 입력/skip·LLM/API/지원 구독/local·default model/effort·Google sender 선택·접속 mode를 묻는다. 외부 연결을 건너뛰어도 관리 화면은 열리고 추가 계정은 필요할 때만 만든다. 설정/비밀을 stdout/log/URL/argv로 노출하지 않는다. 최초 임시 비밀번호는 인증된 setup UI/controlling TTY에 한 번 표시하는 [설치 계약](onboarding.md)을 따른다. 최소 bootstrap script가 필요하면 release 검증/실행만 하며 제품 비즈니스 로직은 Rust에 둔다. optional gws/Claude CLI는 별도 설치와 지원 범위를 표시한다.

Tailscale Serve는 tailnet 접근, ordinary HTTPS는 공개 주소 접근이다. 둘 다 app account 인증을 쓴다. Funnel을 Serve 실패의 자동 우회로 켜지 않는다. 기존 DNS/vhost/cron을 installer가 덮어쓰지 않는다. vault는 별도 origin·서비스·데이터 root다. doctor가 private 경로 권한·origin/HTTPS·OAuth callback·SSE proxy·Push·backup/journal·clock·SQLite local FS를 확인한다.

업데이트는 manifest/hash 검증→admission/drain→backup·journal 확인→forward migration→health/contract probe→traffic 재개다. uninstall 기본은 실행 파일/서비스만 제거하고 data 보존, 데이터 삭제는 범위를 명시한다. secret과 DB는 Dropbox/Git/served static root 바깥이다. 원래 host-local 파일/secret을 제거한 환경에서 문서에 명시한 offline backup identity·archive·최신 journal/검증 자료만으로 restore가 성공해야 복구 지원으로 표시한다. backup key는 사용자 vault master와 분리하며 교체 전후 archive/key 보관도 검증한다. restore는 옛 password/role/provider를 자동 재활성화하지 않고 로컬 operator recovery와 credential 재등록을 요구한다. snapshot 이후 사용량이 불명이면 해당 예산의 새 지출을 막으며, 일반 재시작에는 이 절차가 없다.

## 출시 gate

| 영역 | 필요한 증거 |
| --- | --- |
| 공개 파일 | 새 이력·허용 목록·gitleaks 등 secret scan·개인 문자열/원문 수동 검토·asset/license provenance |
| 재현 | 고정 Cargo/web lock·clean build·format/lint·SBOM·release checksum/signature·명시 MSRV/platform |
| 서버·격리 | Rust 계약/실제 SQLite WAL/kill/lock/backup/restore·두 사용자 모든 객체·fair budget·회수/unknown effect |
| React/PWA | typecheck/component/browser·account switch·모바일 safe area/scroll/IME·attachment/녹음/retry·실기기 Push/share/install |
| LLM | adapter별 synthetic stream/tools/capability/usage·OpenRouter route/local offline·공식 auth·지원 model 선택 |
| 볼트·broker | v1 패키지 제공(사용자 설치 선택). 합성 crypto·no escrow·rotation/restore·최소 지원 API/Google adapter·local grant/online commitment/철회·실행 방식별 악성 서버 한계 |
| 운영 | fresh install→admin→초대→강제 변경→두 사용자→update→backup/restore→uninstall(data 보존) 필수 사진/PDF 포함 빈 data_dir 복원·실제 Linux 시험 |
| 품질·속도 | 실행 가능한 pinned baseline·independent holdout·실패 포함 분모·신뢰구간·추정/실측 분리·first-final-body/queue/total·기존보다 퇴행 여부 |
| 문서·지원 | README quickstart, admin/users/providers/vault/backup·troubleshooting·SECURITY/CONTRIBUTING·issue/PR templates·unsupported 표 |

무료 API quota·특정 subscription/model 가용성을 보장하지 않는다. local model 설치·GPU는 사용자가 제공한다. 선택 connector가 미구성/기능 제한일 때 UI는 숨겨진 오류 대신 unavailable/재연결 상태를 표시한다. 합성 시험 통과를 실계정·실기기 검증으로 바꾸어 쓰지 않는다. GitHub 공개 직전 최종 허용 목록을 다시 고정한다.

## 기여와 community 배포

[CONTRIBUTING](../CONTRIBUTING.md), [SECURITY](../SECURITY.md), issue/PR 템플릿과 고정 OpenSpec CLI를 준비한다. 사용자 지시로 초기 GitHub Actions/자동 CI를 비활성화한다. workflow는 tools/ci/check.yml.disabled에 보관하며 로컬 계획/spec/구현 검증을 작업 단위마다 실행한다. 재활성화는 별도 지시를 따른다. 제품 runtime/React/모델 시험을 실행한 것처럼 표시하지 않는다. maintainer는 @codemoo이며 비활성 CI를 필수 branch check로 설정하지 않는다. GitHub private vulnerability reporting을 공개 저장소에 활성화한다.

community source는 같은 Git repository의 `community/skills/`에서 시작한다. trusted release가 검증한 정적 catalog를 생성하고 사용자는 기존 제안/할 수 있는 일 카드에서 추가·연결·실행한다. package source/version/hash/license/capability를 추적하며 외부 Git URL은 고급 경로다. [상세 배포 설계](community-skills.md)에 따른다. 별도 marketplace 계정/서버·자동 코드 실행·private learned trace 업로드는 도입하지 않는다.

출시 평가는 [testing](testing.md)의 새 시나리오·독립 판정·paired 비교·실제 기기/연결 증거를 따른다. credential 지원은 [credential-use](credential-use.md)의 strong-device와 명시적 server-token 경계를 구별해 표시한다.
