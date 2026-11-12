# 현재 상태

2026-11-12. 실제 구현·검증과 다음 작업은 이 문서가 정본이다. 기능 계약은 [features](features.md), 상세 단계는 [plan](plan.md)을 따른다.

공개 저장소는 [codemoo/moosuri](https://github.com/codemoo/moosuri)다. **계정 기반을 구현한 개발 프리뷰이며, 아직 대답하거나 일을 수행하는 비서는 아니다.** GitHub Actions/CI는 사용자 지시로 꺼져 있다. 작업 단위마다 로컬 검증 후 커밋·푸시한다.

## 구현된 첫 단위

| 영역 | 실제 동작 |
| --- | --- |
| 실행 | Rust 1.89 CLI/API, React 정적 파일을 포함한 서버, SQLite 단일 writer, Linux 실행 |
| 초기 설치 | 숨김 비밀번호 입력 또는 owner-only 파일, 최초 admin 한 명, 비워 두면 일회성 임시 비밀번호 |
| 회원 | admin의 이름·이메일 기반 로컬 초대, 24시간 임시 비밀번호, 최초 변경 전 개인 API 차단 |
| 세션 | HttpOnly cookie, 서버의 30일 유휴 만료·활동 갱신, 비밀번호 변경 시 이전 세션 철회 |
| 사용자 격리 | 세션에서 정한 tenant, 매 작업 재검증, admin도 타인의 대화 접근 불가 |
| 웹 | 로그인·비밀번호 변경·초대·회원 목록, 빈 대화 생성·목록·삭제, 모바일 별도 스크롤 |
| 탭 전환 | 이전 계정 요청 중단/결과 폐기, expected-account 검사, 탭 간 갱신, 늦은 로그아웃의 쿠키 덮어쓰기 방지 |
| 저장 | private 파일·경로 검증, 프로세스 writer lock, bounded queue/KDF, auth epoch journal의 중단·잘림 탐지 |

설치·빌드·프록시·로컬 검증 명령은 [development](development.md)에 있다. 초대 이메일은 발송하지 않으며 대화는 아직 빈 컨테이너다. 모델이 연결되거나 메시지가 전송된 것처럼 표시하지 않는다.

## 검증한 범위

- Rust 1.89에서 fmt·clippy(`-D warnings`)와 **17개 테스트** 통과. 파일 DB·별도 프로세스 writer 경쟁·임시 비밀번호 교환 경쟁·tenant 경계·journal 중단을 검사한다.
- macOS와 Linux x86_64의 격리 Docker 환경에서 같은 17개 테스트 통과. 실제 CLI 초기화→내장 웹→HTTP 로그인→재시작→세션 유지→대화 삭제까지 포함한다. 운영 서비스 배포는 하지 않았다.
- React의 **18개 행동 테스트**, TypeScript 검사와 production build 통과. Rust에서 생성한 웹 DTO 일치 확인.
- Chromium 실제 서버 여정과 두 탭 회귀 검증의 범위는 [계정 기반 검증 기록](reviews/2026-11-12-account-foundation.md)에 기록한다. 모바일 viewport 확인은 실제 iOS/Android 검증과 구별한다.
- 새 소스의 인증·저장·브라우저 경계를 독립 검토했다. proxy/NAT 로그인 제한 공유, 잘못된 디렉터리 권한 변경, 다른 탭 계정 혼동, 늦은 로그아웃 응답 문제를 수정하고 재검토했다.
- SQLite advisory에 따라 rusqlite 0.41.1/libsqlite3-sys 0.39.1로 갱신하고 DB open 전에 SQLite 3.54 이상을 확인한다. 최종 Cargo.lock의 registry 패키지 153개 OSV 조회에서 알려진 취약점 응답 없음. 이는 취약점 부재 보증이 아니다.

기존 테스트는 복사하지 않았다. 새 테스트도 전체 제품 평가 성적으로 사용하지 않는다. source523·entrypoint108·R28/F69 조사표, 67개 planned 평가 묶음과 과거 검토 기록은 유지한다.

## 아직 구현하지 않은 범위

공용 provider 연결/모델·effort 설정, 메시지 저장·스트림·재시도·취소, 에이전트 하네스·도구·기억·평가, 예약·알림·푸시, 첨부·녹음·PWA, 수집 connector, Google/외부 작업, community 스킬, 독립 볼트·TOTP·credential broker, 백업·복원·데이터 이전·일반 설치 패키지는 남아 있다. 볼트는 v1 제공 범위이며 사용자 연결 여부만 선택 사항이다.

현재 auth journal과 DB 사이 crash gap은 해당 계정을 `recovery_required`로 잠근다. 이를 해제할 operator 복구 경로와 백업/복원은 아직 없으므로 운영 계정·자료를 이 개발 프리뷰로 옮기지 않는다. `doctor`는 중지된 설치에만 사용한다.

## 다음 작업

1. P0의 남은 provider capability·baseline/fixture adapter·crypto/broker 위험 검증과 독립 복구 입력을 확인한다.
2. P1의 run/effect·dispatch·revision·사용량·복원 계약을 작은 저장 단위로 구현한다. 현재 계정 기반이 P1 전체 완료를 뜻하지 않는다.
3. P2의 provider 설정과 두 사용자의 실제 텍스트 대화·스트림·취소·재시도·재접속 흐름을 잇는다.
4. [migration](migration.md)의 순서에 따라 자료·기억→도구·스킬→예약·연결→미디어·볼트→이전·출시로 진행한다. P0/P1/P2와 미구현 OpenSpec change를 완료 처리하지 않는다.

모든 공개 소스는 새 이력과 합성 자료만 사용한다. 원본 운영 저장소·서비스·수집기·cron·DB는 보존한다. 개인 AGENTS/CLAUDE 및 editor 지침은 Git에서 제외하고 CONTRIBUTING·OpenSpec·제품 community SKILL은 공유한다.

## 유지되는 설계 결정

백업 manifest·첨부 pin·서버 밖 outer recovery key, DST/중단 예약 규칙, current epoch의 dispatch/철회 순서, 사용자 기기의 credential grant, 증거 없는 외부 작업 성공 금지와 평가의 not-run 구분은 그대로 유지한다. 계약 검토 기록은 [초기 검사](reviews/2026-11-12-initial-checks.md)와 [사용자 여정 검토](reviews/2026-11-12-user-journeys.md)에 있으며 실제 암호/복원/provider 시험과 구별한다.
