# 표준 서버의 브라우저·절차·기여 여정 검증

2026-11-20, macOS arm64와 격리된 Linux x86-64에서 합성 자료를 사용했다. [기능별 대장](feature-journeys.tsv)은 전체 제품 기능표의 71개 기능에 3~4건씩, 총 217개 시나리오를 둔다. 각 행에는 조작과 기대 결과, 실제로 검사한 구현 경로와 실행 상태가 있다. 두 회원 화면 체계 중 어느 쪽을 확인했는지는 해당 행의 소스 경로를 따른다. [기능표](../docs/features.md) · [Kiri 전용 조작 기록](../docs/reviews/2026-11-20-kiri-scenarios.md)

| 결과 | 건수 | 확인 범위 |
| --- | ---: | --- |
| 통과 | 196 | 웹·볼트의 Vitest 결과에 연결된 102행, Rust 테스트에 연결된 82행, 직접 수행한 실험·절차 12행 |
| 부분 확인 | 7 | OS별 재설치, 실물 휴대폰 키보드·푸시, 실제 Google 동의, 기존 개인 수집기 공존, 외부 모델 공급자·로컬 엔진 조합 |
| 실환경 대기 | 11 | 전용 메일·달력, Slack·Telegram, 기기 전사·인증, 실제 배송물, tailnet, 서명 배포·독립 설치와 외부 기여 |
| 후속 범위 | 3 | F43의 차량, 경로, Todoist·로컬 전사 |

웹 테스트는 771개, 볼트 웹은 18개가 통과했다. 위 196은 사용자 시나리오 수다. Linux 워크스페이스 전체를 한 번 실행한 결과 1,678개 통과, 45개 실패, 9개 기본 생략이었다. 실패한 45개를 이름별로 추적해 제품 또는 테스트 환경을 수정한 뒤 모두 다시 통과한 것을 확인했다. 수정 후에는 변경된 경로를 선별 실행했다. [실행 결과 대조 방법](../docs/testing.md#기능별-시나리오-실행-기록)

## 표준 웹 실제 브라우저

전용 임시 설치와 실제 Chromium으로 로그인, 합성 모델 스트림 수신, 새로고침 후 검색으로 원래 대화 복귀, 소식함·예약 화면을 거친 후 초안 복구, 조용한 시간 변경·새로고침을 확인했다. 다섯 여정이 통과했다. 앱·SQLite와 HTTP/SSE 경로를 사용했고 공급자만 합성했다. 모델 준비 검사 1회와 실제 회원 질문 호출 1회가 기록됐다. [실행기](../web/tests/standard-web-fullstack.py)

```sh
python3 web/tests/standard-web-fullstack.py target/release/moosuri
```

## F12 데스크톱 사진 준비

웹의 실제 `prepareImage` 모듈을 Chrome 158.0.8127.36의 JPEG 디코더와 캔버스 인코더로 실행했다. 생성한 JPEG의 높이는 모두 6000px다.

| 입력 너비 | 입력 바이트 | 실제 결과 |
| ---: | ---: | --- |
| 7999 | 33,554,431 | JPEG 2400×1800, 26,183바이트 |
| 8000 | 33,554,432 | JPEG 2400×1800, 26,183바이트 |
| 8001 | 33,554,432 | 48MP 한도 오류 |
| 8000 | 33,554,433 | 32MiB 한도 오류 |

`npm --prefix web run dev`로 서버를 열고 브라우저에서 `/tests/media-preparation.browser.js`를 연 뒤 개발자 콘솔에서 아래 식을 실행하면 네 입력에 대한 단언과 결과를 재현할 수 있다. 실물 휴대폰 검증은 별도다.

```js
await (await import("/tests/media-preparation.browser.js")).verifyPhotoLimits()
```

## F25 절차 선택

연결이 누락된 카드는 준비됨으로 표시하지 않았다. 합성 공급자가 카탈로그 요약에서 비교 절차의 리비전·해시를 선택했다. 첫 호출에는 절차 본문이 없었고, 선택한 절차 읽기 후 실제 저장 자료 두 건을 읽고 정확한 합계를 계산했다. 다른 해시와 존재하지 않는 절차를 읽으려는 Store 요청은 모두 거절됐다. 세 명령에서 각각 지정한 테스트 한 개가 통과했다.

```sh
npm --prefix web test -- src/CommunitySkills.test.tsx -t 'does not advertise missing connections as ready and persists hiding through stable package action'
cargo test --locked -p moosuri-agent --lib delegated_comparison_delivers_evidence_and_exact_totals_without_external_effects
cargo test --locked -p moosuri-store --lib profile_and_skill_receipts_survive_retry_without_upgrading_legacy_runs
```

## F25 공개 절차 출처

`crates/moosuri-core/src/skills.rs`가 기본 제공하는 본문 8개의 사용 조건과 한도를 확인했다. 개인 자료를 읽는 세 절차는 저장된 근거에 한정하고, 받은편지함 요약과 회신 초안은 회원 Gmail 연결을 요구한다. 웹 조사와 공개 페이지 비교는 조회한 공개 자료를 근거로 삼고, 배송 조회는 일본우편 결과만 취급한다. 설치 가능한 공개 패키지 6개의 `moosuri skills check`가 통과했다. 각각 선언한 capability 수는 evidence-comparison 4개, inbox-briefing 3개, mail-reply 4개, meeting-preparation 3개, source-briefing 3개, web-research 2개였다.

아래 검사는 제외 대상으로 기록된 파일의 길이와 SHA-256만 사용한다. 공개 패키지 26개 파일 안에 52개 제외 원본 중 하나의 전체 바이트열이 들어간 위치는 0곳이었다. 부분 인용이나 재서술 여부는 이 지문 검사의 측정 범위를 벗어난다.

```sh
python3 audit/verify-public-recipe-origins.py
for recipe in evidence-comparison inbox-briefing mail-reply meeting-preparation source-briefing web-research; do
  cargo run --locked -p moosuri -- skills check "community/skills/$recipe" || exit 1
done
cargo test --locked -p moosuri-store --lib six_packages_preserve_curated_pins_and_generate_deterministic_existing_v2_envelopes
```

## F65 변경 제안 확인

OpenSpec 2.0.0은 `add-community-skill-discovery` 명세의 엄격 검증을 통과했다. 연결 누락 시 준비 상태를 표시하는 시나리오와 업데이트 중단 시 이전 버전을 유지하는 시나리오를 확인했다. 작업표에서 준비·구현 5개가 완료됐고 비공개 학습 내보내기 및 공개 수락 2개가 열려 있었다.

```sh
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec validate add-community-skill-discovery --strict
OPENSPEC_TELEMETRY=0 tools/openspec/node_modules/.bin/openspec instructions apply --change add-community-skill-discovery --json
```

## F65 계획 검사

새 체크아웃에서 `cargo run --locked -p xtask -- verify-plan`이 소스 523개, 진입점 108개, 요구사항 30개, 기능 71개, 계획 시나리오 그룹 71개, 로컬 링크 1459개를 확인하고 종료 코드 0을 반환했다. 동일 checkout의 `docs/` 아래에 임시 Markdown 파일을 하나씩 생성해 같은 검사기에 넘겼다. 다음 두 입력 모두 예상한 이유로 종료 코드 1을 반환했다. 각 입력은 검사 후 제거했다.

| 임시 문서 내용 | 관측한 오류 |
| --- | --- |
| `[local reference](missing-synthetic-plan-link.md)` | `missing link missing-synthetic-plan-link.md` |
| `Rust/WASM 웹` | `stale assumption` |

## F18 합성 미리보기

회원과 표준 서버의 `/demo`는 정적 시간표·기억·작업 응답 세 개를 제공한다. Chrome의 390px와 1200px에서 예제를 바꾸고 연습 초안을 작성한 뒤 **회원 공간 열기**를 눌렀다. 데모는 개인 API를 요청하지 않았고 브라우저 저장소는 비어 있었다. 회원 로그인 화면에서 세션 조회가 시작됐으며 예시를 다시 열었을 때 초안은 비어 있었다. 아래 이미지는 데모의 합성 시간표 화면에서 캡처해 예제 문장·승강장·브랜드만 담긴 것을 확인했다. [공개용 합성 화면](../docs/reviews/2026-11-20-public-demo.png) · [예시 데이터](../web/src/PublicDemo.tsx)

## F53 스키마 47에서 90으로 서명 업데이트

공개 커밋 `026e315`를 당시 웹 의존성과 함께 빌드해 스키마 47의 실제 선행 실행 파일로 사용했다. 격리한 Linux에서 서명된 업데이트를 시작하면 구버전 백업이 Telegram 테이블을 요구해 중단되는 결함이 재현됐다. Telegram이 도입된 스키마 75부터 복사본의 자격 증명을 격리하도록 고쳤다. 더 이른 버전에 예상 밖의 Telegram 테이블이 있으면 보관 파일이나 다음 활성화 상태를 쓰기 전에 거절한다.

수정 후 마이그레이션 시작 직후·완료 직후·관리자 전환 직후·현재 버전 전환 직후에 각각 중단하고 재시도했다. 네 경우 모두 새 버전으로 복구했고 이전 실행 파일로의 되돌림은 차단됐다. 업데이트 전후에 생성한 계정 자료와 암호화 백업이 보존됐다. 별도 Go `age`로 이전 백업을 복호화해 보관 파일 목록·해시·길이, 실제 SQLite 스키마 47, 무결성과 외래 키를 확인했다. 현재 스키마의 구성된 합성 Telegram 토큰은 백업에서 제거됐다. [업데이트 수락 테스트](../apps/moosuri/tests/schema_update.rs) · [백업 회귀](../crates/moosuri-store/src/backup/tests.rs)

```sh
MOOSURI_TEST_PREDECESSOR_BINARY=/previous/moosuri cargo test --locked -p moosuri --features test-fixtures --test schema_update -- --ignored --test-threads=1
```

## F38 공개 배포 경계

`git archive b292bd1`의 파일 2,289개를 읽고 역사 자료 목록 중 F38 또는 Kakao와 연관된 스크립트·코드의 고유 SHA-256 11개와 대조했다. 전체 내용이 일치한 파일은 0개였고 Kakao·카카오·Talk.db가 들어간 배포 경로도 0개였다. 설치 스크립트, Rust 제품 코드, 배포 설정과 웹 실행 코드에서도 이 서비스의 전용 도구 등록을 찾지 못했다. 커넥터 목록의 Kakao 정책은 공개 제외다. [커넥터 정책](source-connectors.tsv) · [원본 지문](source-inventory.tsv)

개인 생산자와의 공존은 로컬에서 합성 SQLite와 일정 표식 파일을 공개판의 데이터 경로 밖에 둔 상태로 검사했다. 공개 CLI로 설치 계정을 만든 후 두 파일의 내용·inode·수정 시각이 보존됐고 원래 DB 연결로 두 번째 기록을 쓸 수 있었다. 실제 개인 수집기·운영 DB·cron은 이 시험의 대상에서 제외했다.
