# 표준 서버의 브라우저·절차·기여 여정 검증

2026-11-20, macOS arm64. 검증에 합성 자료와 공개 저장소를 사용했다. 아래 제목은 기능표의 `recorded` 행에서 참조한다.

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
