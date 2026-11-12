# 현재 상태

2026-11-12. 실제 구현·검증과 다음 작업은 이 문서가 정본이다. 기능 계약은 [features](features.md), 상세 설계는 각 범위 문서를 따른다.

| 항목 | 상태 |
| --- | --- |
| 제품 | Rust backend/agent/CLI + React web/PWA/admin + SQLite, Linux 단일 호스트 |
| 구현 | 제품 runtime 미구현. Rust 계획 검증기와 OpenSpec 구조 검증 도구만 실행 가능 |
| 조사·추적 | source523·HTTP59/tool49·R28/F69. 기존112 test/support는108 reference·4 exclude |
| 평가 | 67개 새 시나리오 묶음, 전부 planned. 모델/미션 성적·제품 시험 통과 주장은 없음 |
| 공동 개발 | CONTRIBUTING/SECURITY/issue·PR 준비. 사용자 지시로 CI 비활성, 작업 단위별 로컬 검증·commit/push. OpenSpec2.0.0 baseline3개·active changes4개 |
| 볼트 | v1 제공, 사용자가 설치/연결 선택. 비밀 비노출·기기 grant와 서버에 위임한 작업 범위를 구별 |
| 공개·운영 | codemoo/moosuri 공개 개발 저장소 준비·MIT OR Apache-2.0. 개인 agent 지침은 Git 제외. 기존 서비스·수집기·cron·DB·사용자 수정 보존 |
| 검토 | 이전6건 보완 유지. 추가 사용자 여정 검토에서 outer backup key 보관 누락1건 보완·독립 재검토 완료 |

## 전체 재검토에서 보완한 계약

- **복원:** DB와 필수 첨부/산출물의 snapshot manifest·GC pin·암호화 archive를 한 백업으로 검증한다. 빈 data_dir에서 파일 누락/변조를 발견하면 복원 완료가 아니다.
- **복구 키:** outer backup identity는 운영자가 서버 밖에 보관한다. 사용자 vault master와 분리하고, key 교체 전후 archive/journal의 old identity 및 검증 자료를 유지한다. 원래 호스트/복원할 vault 없이 복원할 수 있어야 한다.
- **예약:** DST gap/fold·UTC 간격·중단 중 latest-only 실행·pause 기간 skip·수정 revision과 dispatch 순서를 정했다.
- **품질 비교:** P0에 실행 가능한 pinned baseline과 공통 합성 fixture adapter를 확인한다. baseline이 없으면 비열등 비교는 not-run이며 과거 점수로 대체하지 않는다.
- **v1 범위:** 볼트와 지원 broker는 출시 제공 대상이다. 선택은 사용자 설치/연결 선택이며 임의로 후속 범위로 미루지 않는다.
- **credential 신뢰:** 기기가 local grant의 최대 범위를 소유한다. v1은 그 안에서 요청을 구성하는 서버를 신뢰하며 비밀 비노출을 악성 서버의 사용자 의도 위조 방어로 과장하지 않는다.
- **철회:** 현재 epoch의 online dispatch commitment와 철회를 순서화한다. 이미 committed된 작업은 in-flight/unknown, 기기 철회 ACK 미확인은 pending이다.

공개 개발 저장소는 P0의 권리/비밀/새 이력 gate 뒤 준비하고 안정판은 P10이다. 초기 임시 비밀번호는 인증된 setup UI/TTY로1회 전달하며 로그 출력과 구분한다. 과거 수치·작업별 검증 기록은 [이력](reviews/2026-11-12-initial-checks.md)에 보존했고 CLAUDE 진입점의 중복 규칙을 제거했다.

[사용자 여정 검토 기록](reviews/2026-11-12-user-journeys.md)은 검토한 반례와 판단을 정리한다. 실제 제품 시험 성적으로 쓰지 않는다.

## 다음 구현 순서

1. **P0:** Rust/React 최소 빌드, SQLite·하네스·crypto/broker 위험 spike, baseline/fixture adapter·backup key format/독립 복구 입력, 실제 provider capability와 공개 라이선스 결정.
2. **P1:** tenant repository·계정/예산·run/effect·dispatch commitment·journal·immutable file/backup pin의 계약 구현.
3. **P2:** 두 사용자의 실제 text 대화·모델 설정·스트림·재시도·삭제·재접속까지 alpha 수직 흐름.
4. 이후 [migration](migration.md)의 domain gate를 통과해 P10 안정판으로 진행한다. 미구현 OpenSpec change는 archive하지 않는다.

## 검증 상태와 한계

제품 여정·복구/격리·출시/평가의 독립 검토3개를 완료했다. 추가 지적은 outer backup key 보관 누락1건이며 수정본의 좁은 독립 재검토에서도 문서 계약상 해결을 확인했다. 실제 암호화 구현과 복원 시험을 통과했다는 뜻은 아니다.

이번 수정본은 Rust 계획 검증기에서 source523·entrypoint108·R28/F69·67개 planned 시나리오·151개 로컬 링크를 확인했고, 고정 OpenSpec2.0.0 strict는7/7 통과했다. 원본 source/entrypoint/manifest/test-surface 조사표와 검증기 코드는 유지했다. 앞선 fmt/check·DST UTC 계산 등은 당시 검증 증거이며 기존 기록을 따른다.

GitHub CI 실행·Linux 설치·SQLite kill/restore·실제 모델·React/PWA/마이크·새 crypto/broker 시험은 아직 하지 않았다. 외부 자료는 [references](references.md)의 확인 범위를 따르며 이번 구조 검토를 새 실계정/provider 검증으로 표현하지 않는다.
