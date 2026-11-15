# 원래 맡긴 일의 누락 값 이어하기 검증

2026-11-15, 합성 자료와 전용 SQLite 및 loopback fixture만 사용했다. 실제 계정/운영 서비스/외부
provider를 변경하지 않았다. 실모델의 질문 이해 품질이나 실제 Gmail/Calendar 준비 상태의 수락은 아니다.

## 확인한 사용자 결과

- 완료한 계산 뒤 수신인 질문으로 대기하면 worker/runnable 자리를 반환한다. 답변을 저장한 뒤 다른
  업무가 끝나면 같은 run과 원래 acceptance/floor/profile/plan/receipt/counters/deadline으로 돌아온다.
  응답 유실로 같은 답변을 다시 제출해도 MailAction 하나만 준비되며 발송은0건이다.
- 완료한 Gmail/Calendar 읽기 뒤 원래 카드가 수신인을 묻는다. 엔진을 멈추고 재개한 뒤에도 질문은
  보관되고, 대기 중 추가 모델 호출은0회다. 정확한 Gmail 계정 선택 주소는 받는 사람으로 오해하지
  않는다. 답변 뒤 읽기를 반복하지 않고 기존 메일/Calendar 작업 두 개를 준비해 총6모델 호출 안에 끝낸다.
- 입력 ready와 연결 ready가 동시에 있어도 명시적으로 멈춘 다른 작업의 자리를 빼앗지 않는다.
  자리 반환 뒤 기존 접수 순서대로 각각 원래 작업을 승격한다.
- 잘못된 owner, 자유 명령이 붙은 clock, 같은 UUID의 다른 payload, 이미 명시한 값의 교체,
  같은 field 재질문과 원래6호출 소진 뒤 새 질문을 거절한다. 기다린 중 만료하면 재개하지 않는다.
- 빠진 Calendar 시작 시각을09:00으로 답하면 기존 Calendar ledger에09:00–09:30의 정확한 작업과
  합성 receipt를 남긴다. 이미 주어진 시작·종료 범위를 duration 답변으로 바꾸지 않는다.
- schema68→69 일반 열기와 restore migration을 확인했다. ready 답변이 있는 암호화 snapshot을
  복원하면 입력/답변 권한을 제거하고 원래 업무를 quarantine하며 자동 실행하지 않는다. 삭제하면
  질문/답변도 지운다.

## 실행한 검사

- `cargo test --locked -p moosuri-store mission_input -- --nocapture`:7개 통과.
- `cargo test --locked -p moosuri-agent mission_input_runtime -- --nocapture`: 원래 계정 선택→읽기→질문→엔진 재시작→같은 작업의 초안/일정 경로 통과.
- `cargo test --locked -p moosuri-agent compound_actions_runtime -- --nocapture`: 기존 연결 작업 통과.
- `cargo test --locked -p moosuri-store compound_ -- --nocapture`: 최초19통과/1실패. 실패는 schema58을
  흉내 낸 fixture가 schema61–68 테이블을 남긴 상태였다. 실제 과거 schema가 되도록 그 테이블들을
  제거한 뒤 `compound_report_schema58` 한 검사만 재실행해 통과했다. 통과한19개는 반복하지 않았다.
- `cargo clippy --locked -p moosuri-server --all-targets -- -D warnings`, `cargo fmt --all`: 통과.
- React typecheck/production build와 `MissionInput.test.tsx`, `Missions.test.tsx`:22개 통과.
- 현재 CLI의 `export-types`로 공유 DTO를 생성했다. pinned OpenSpec2 strict 및 `xtask verify-plan` 통과.

최종 통합 소스를 격리한 Linux x86_64 컨테이너에서도 검증했다. 외부 네트워크 없이 Store
`mission_input` 7개, Runtime `mission_input_runtime` 1개와 `credential-broker-chat`의
`native_mail_runtime` 1개가 모두 통과했다. 서버의 `credential-broker-foundation` 기능을 켠
Clippy도 통과했으며 실행 전후 소스 해시가 일치했다. 해당 소스 아카이브 SHA-256은
`968ed8d2e90579b58b2b9b3e512a83a95032a52144db977164bf32003a36b6c3`다.
통합 checkout의 React 검사22개와 typecheck/production build도 통과했고, 현재 CLI가 생성한
공유 DTO와 커밋할 파일이 정확히 일치했다. 운영 배포나 실계정 검증은 수행하지 않았다.

공유 native target이 다른 checkout의 core DTO metadata를 재사용한 오류는 해당 package만 clean한
뒤 해결했다. 초기 검사에서 restart 관측 timing까지 profile로 비교하거나 Calendar preflight GET을
읽기 반복으로 센 부정확한 시험 oracle도 고쳤다. 결과 동작과 계정/작업 identity를 기준으로 검증했다.

## 독립 검토에서 고친 경계

최초 frozen 검토는 두 문제를 지적했다. 완전한09:00–10:00 원래 범위를30분 질문으로 교체할 수 있던
경계와, Gmail account 선택 주소를 수신인으로 오해해 필요한 질문을 거절하던 경계다. 지원한 exact
계정 선택 발생 위치만 구별하고 원래 range/길이의 질문을 보수적으로 거절했다. 첫 range 보완은
`오전9시부터10시까지`의 오전 상속을 놓쳤다. 추가로 명시한 range cue를 보수적으로 거절하고
실제 기존 Calendar ledger의 원래 종료10:00을 검증한다. 일반 시간 parser나 쓰기 권한을 확장하지 않는다. 두 문제는 R2의 frozen 독립 경계 검토에서
닫혔으며 새 미해결 P1/P2는 없었다. 전체 제품의 보안 감사나 출시 수락을 뜻하지 않는다.

## 남은 범위

일회성 이미 허용된 작업의 단일 수신인/날짜/시각/길이만 지원한다. 임의 자유 답변, 보고서/원문
충돌의 선택, Calendar 대상 모호함, 반복 작업은 후속 범위다. 한 번 맡긴 일의 일반 action-needed
알림 경로는 없으며 질문은 원래 카드에 보관한다. 메일마다 정확한 최종 발송 허락은 계속 필요하다.
