# 초기 계획 검증 기록

이 문서는 당시 결과를 보존한 이력이다. 현재 판단은 [status](../status.md)를 따른다.

## 이전 계획 패키지 검증 · 2026-11-12

- Rust1.88: cargo fmt --check, cargo check --offline --locked --all-targets, xtask verify-plan 통과.
- source523/entrypoint108/test source99/R22/F64/로컬 링크49개 일관성 확인. 잘못된 source 누락·hash·요구사항 누락·링크4종이 verifier에서 실제 거절되는 것도 별도 복사본으로 확인했다.
- 기존523 source의 고정 SHA256 재대조 결과 변경0. 구체적인 개인 경로/domain/name/대표 key 패턴 검사 결과 반입 탐지0이며 완전한 secret/security audit 보장은 아니다.
- 독립 검토가 지적한 restore 시 옛 password/role/provider 재활성화와 사용량 롤백 문제를 수정하고 좁은 재검토를 통과했다. 실제 runtime 보장은 P1/P9 구현 시험 대상이다.
- 현재 제품 배포·GitHub publish·개인 credential 로그인·모델 유료 호출·기존 DB/cron 변경은 수행하지 않았다.

## 추가 계획 패키지 검증 · 2026-11-12

- Rust1.88: `cargo fmt --all --check`, `cargo check --offline --locked --all-targets`, `xtask verify-plan` 통과. 현재28 requirements/69 features/67 planned scenario groups/128 local links를 확인했다.
- 새 verifier에 기능 시나리오 누락·중복 ID·unknown F ID·잘못된 단계·제품 passed 허위 표시·nested OpenSpec 링크 누락·community 시나리오 오타·legacy test 자동이식 판정8종을 주입했고 모두 거절했다. 별도 복사본을 복구한 뒤 정상 통과도 확인했다.
- Node22.22.0과 repo-local OpenSpec2.0.0: `npm ci --ignore-scripts`, strict validation6/6 통과. 전역 CLI는 변경하지 않았다. CI/issue/config YAML 구조도 확인했다. `.github/workflows/check.yml`은 아직 GitHub에서 실행되지 않았다.
- 두 독립 Sol/high 검토에서 평가 계획은 중대 지적 없음. credential effect의 기기 간 중복 재발송 가능성1건을 broker identity/generation 고정·자동 failover 금지·CRED05로 보완하고 좁은 재검토를 통과했다.
- 공개 계획 파일60개의 개인 경로/식별자/대표 credential 패턴 검사에서 탐지0. 완전한 secret/security audit을 뜻하지 않는다. 원본523개 digest는 개인 도메인 파일명1개의 공개 alias mapping을 고려해 변경0을 확인했다. source manifest는 그대로 보존했다.
- agent/credential broker/community installer/모델 평가 runtime은 미구현이다. GitHub publish·실계정/볼트 접근·유료 model 호출·서비스/DB/cron 변경은 수행하지 않았다.

## 이전 제품의 참고 수치

기존 Rust 이전 전의 최신 회귀는 backend1,121·UI150개였다. 모델 비교는40종×3회에서 기존103/120·개선110/120이며, 일부는 좁은 언어 probe다. 이 숫자는 새 Rust 제품의 시험 결과가 아니다. 마지막 계산량 제한 변경이 모델 비교 뒤였다는 한계도 유지한다.
