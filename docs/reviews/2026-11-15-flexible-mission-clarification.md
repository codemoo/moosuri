# 원래 업무의 정보 보완·선택·회차 대기·소식 검증

2026-11-15, 기준 `fe3248c`에 적용한 비공개 feature checkout의 합성 검증이다.
P4/P5/P6의 기존 미션·회차·알림 계약을 확장하며, 실제 계정/provider/Push 기기 수락이나
실모델 이해 품질, 공개·배포·운영 이전의 증거가 아니다. CI는 변경하지 않았다.

## 사용 가능한 결과

- 원래 업무 카드에서4000자 이내 맥락·판단 기준, 현재 읽은 저장 원문2–5개 중 선택,
  기존 날짜 이동이 허용된 동일 제목 Calendar 대상2–5개 중 선택을 답한다.
- 같은 mission/run·acceptance·완료 receipt·모델/profile·호출/보완 한도와 원래 종료 시각을
  유지한다. 답변 대기는 실행 자리를 돌려주고 모델 polling을 하지 않는다.
- 원문 선택은 실제 ID/revision/receipt에, Calendar 선택은 실제 ID/ETag/receipt에 묶인다.
  모델의 표시 이름은 버리고 실제 관측에서 이름을 만든다. 바뀐/철회된 원문, 다른 일정,
  변경된 ETag를 기존 원장에 조용히 대체하지 않는다.
- 현재 반복 읽기 회차도 정보/저장 원문 선택을 기다리며 답변이 다음 회차/template에
  복사되지 않는다. 정지·철회·만료는 기존 회차 권한 경계를 따른다.
- 질문마다 일반 소식 하나를 기존 소식함/opt-in Push에 넣는다. 임시 schedule이나
  reminder occurrence를 만들지 않는다. 업무별 소식 끄기, 반복 업무의 기존 소식 설정,
  조용한 시간과 현재 기기/계정/세션을 따른다. 클릭은 서버가 지금 허용한 mission ID로
  원래 카드에 이동하며 작성 중인 대화 초안을 보존한다.
- 일반 답변은 효과 파라미터 해석에 합치지 않는다. 새 효과·계정·수신인·제목·미래 반복
  권한을 만들지 않으며, 일반 맥락/판단 기준은 결과에서 owner review를 요구한다.

## 관측한 검증

각 필터의 목적과 실제 통과 범위를 기록하며 전체 assistant 품질 점수로 합산하지 않는다.

| 검사 | 관측 |
| --- | --- |
| Store `mission_flexible` |5개 통과. 맥락 대기·동일 답변 ACK·호출/시간 보존·새 효과 없음, 답변 뒤 claimed Push 차단, 실제 원문 receipt/변경된 revision 거절, 선택한 Calendar ID/ETag의 실제 날짜 이동 admission, quiet/mute/unmute의 소식 억제, schema70 populated upgrade |
| Store `mission_input` |7개 통과. 기존 typed 값/공급된 값 보호, 원래 순서와 capacity, 예산·owner·재시작·만료, schema68 복원과 ready 답변 복원 후 미재생 |
| Store `recurring_flexible` |1개 통과. 현재 회차 답변, 다음 회차 입력 비상속/template 보존, recurring grant 철회 후 actionable 질문 소식 없음 |
| Agent `mission_flexible_context_runtime` |1개 통과. 실제 loopback 모델 전송/Store/Runtime에서 완료 계산→질문→engine 종료→동일 답변 재전송→재시작 후 완료 결과. 모델 호출3회에서 대기하고 답변 후1회만 추가, 완료 도구는 다시 실행되지 않음 |
| React/service-worker |MissionInput/Schedules/push-worker29개 통과. typed·일반·선택 UI, 동일 답변 ID 재시도, 원래 카드 링크, server-authorized mission ID와 고정 앱 경로 |
| App 계정 경계 |새 action-needed Push 경로1개 통과. 다른 계정 메시지 무시, 현재 계정 원래 카드 이동과 작성 중 초안 보존 |
| 빌드/계약 |server `cargo check`, CLI `export-types`와 저장된 TypeScript 일치, React typecheck/production build, Rustfmt, OpenSpec2.0 strict 단일 change 검증 통과 |

## 의미 있는 실패와 수정

공유 Cargo target이 다른 checkout의 새 protocol metadata를 재사용해 baseline broker의
exhaustive match 빌드가 먼저 실패했다. source timestamp를 고쳐 해당 의존성 재빌드 후
통과했다. 제품 동작 결함과 구분한다.

schema70에서 notification table을 바꿀 때 단순 deferred FK는 참조 row를 복원해도
COMMIT이 실패했다. 실제 Push 관계와 gapped rowid57을 가진 fixture가 이를 검출했다.
SQLite의 table-rebuild 절차대로 transaction 바깥에서 FK enforcement를 잠시 끄고
원자적으로 교체한 뒤 다시 켜 전체 FK를 검사하도록 수정했다. Store는 검사 통과 전
열리지 않으며, restore는 검증한 비공개 staging DB에서만 수행하고 공개 전에 검사한다.
수정 후 notification ID/body/rowid, Push 참조, mute revision3이 모두 보존됐다.

## 남는 경계

선택 목록은 현재 저장 원문과 허용된 Calendar 날짜 이동 대상이다. 임의 외부 페이지/
Gmail 객체를 위한 새 범용 selector나 새 외부 동작은 추가하지 않는다. 반복 쓰기는
여전히 미지원이다. 일반 답변의 의미·기준 충족은 기계적 성공으로 과장하지 않는다.
이미 provider가 수락한 Push를 회수할 수는 없다. 실제 사용자 기기 수신·실계정 쓰기와
실모델 명확화 품질은 별도 검증이다. 동결 commit `9796cfe`의1617개 파일 해시를 확인한 독립 검토에서 중요한 P1/P2 결함을 발견하지 않았다. 실제 provider/device Push 수락을 대신하지 않는다.

## 통합 수락

통합 checkout의 Linux x86_64 격리 실행에서 `mission_flexible`5개,
`mission_input`7개, `recurring_flexible`1개, 실제 Runtime1개를 통과했다.
원본 archive SHA256은 `aaea61190472ff829dbe71faa7578d7be7b100d4eee4b59fd0714d811c697b31`이며 전송 뒤 파일 해시를 다시 확인했다.
통합 React MissionInput/Schedules/push-worker/App4파일56개와 production build/typecheck도
통과했다. 대상 소스는 독립 검토한 기능 commit과 같으며 공통 상태/계획 문서만 갱신했다.
