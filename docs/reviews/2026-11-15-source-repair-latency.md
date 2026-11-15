# 출처 계산 실패 복구와 호출 감소 — 2026-11-15

기존 W07-B는 요청과 보고서 제목에 같은 검색 문구가 나오는 것을 모호한 입력으로
거절했다. 모델이 검색 문구만 바꾸고 이전 검색 영수증을 재사용하면서 실패했고,
여섯 번의 모델 예산 안에 계산·보고서·두 번째 학습 근거를 만들지 못했다.

새 `source_calculate`는 현재 사용자 query/prefix/scalars와 typed graph만 받아 기존
Store의 실제 검색·전체 원문 읽기·정확 계산을 한 transaction으로 연결한다. 실제 child
영수증과 비용은 보존한다. 같은 query/prefix 문자열을 반복한 요청은 허용하지만 실제
자료 결과, 자료 안 field, 완전 숫자 token과 현재 소유자/revision 검사는 유지한다.
인수·출처 선택·field 오류를 구분하며 이전 실행 profile에 새 도구를 소급하지 않는다.

[첫 실행 최적화](2026-11-15-mission-first-execution.md)는 도구 없이 계획 제목만 생성하던
추론 한 번을 제거했다. 현재 허용된 요청과 완료 step으로 보고서/action 계약을 만든 후
도구를 실행하며 첫 응답 확정 전후·계획 저장·최종 제안 복구를 보존한다.
모델은 양쪽 모두 **gpt-6.1-terra / high**이며 예산을 늘리거나 effort를 낮추지 않았다.

## 고정된 독립 합성 요청 비교

평가자는 공개 계약만 읽고 새 합성7요청과 private oracle을 고정했다. 구현자는 입력만
볼 수 있었고 oracle은 읽지 않았다. 실제 SQLite·Runtime·저장 문서·source revision·산술
child와 owner adoption을 독립 검토했다. 두 실제 A/B 결과의 채택 뒤 새 C의
`learned_skill_execute` 사용, 원문 변경에 따른 옛 문서 철회와 새 금액도 확인했다.

- 원래 v1: 기준7개 실행/7통과. 수정본6개 실행/6통과, 인사1개는 새 provider 설정 probe 실패로 미접수.
- 동일한 핵심5개: 양쪽5/5. 네 보고서는 원문·산술·저장 postcondition 통과이고 의미 검토는 `partial/review_required`로 남는다. 모호한 출처는 선택 대기이며 완료된 보고서로 세지 않는다.
- 별도 v2 보완: 양쪽의 이미 검증한 공유 LLM에 새 빈 회원 계정을 만들어 같은 인사·기억 요청을 실행했다. 사용자 자료·기억·학습 절차가 비어 있음을 API로 확인했고 양쪽2/2 통과했다. 원래 v1 분모·미접수 기록을 덮어쓰지 않았다.
- 이전에 실패한 **노출 사례 W07-B**도 원래 요청·자료로 별도 재실행해 실제 계산·비공개 보고서를 저장했다. 모델2회, 완료 관측9.60초. 새 독립 사례나 완전한 옛 W07 전체 재평가로 세지 않는다.

| 요청 | 이전 → 수정 후 모델 호출 | 이전 → 수정 후 완료 관측 |
| --- | --- | --- |
| 새 원문 보고서 A | 5 → 2 | 22.15 → 9.38초 |
| 독립 원문 보고서 B | 5 → 2 | 23.79 → 12.57초 |
| 학습 절차로 새 C | 3 → 2 | 13.62 → 10.01초 |
| 모호한 출처 질문 | 3 → 3 | 12.90 → 13.16초 |
| 원문 수정 후 재계산 | 3 → 2 | 14.96 → 9.17초 |
| 간단한 인사 (v2) | 1 → 1 | 3.36 → 3.76초 |
| 명시적 기억 저장 (v2) | 1 → 1 | 4.18 → 3.76초 |

각 행은 단 한 번의 짝 비교다. A/B의 호출5→2는 설명용 계획1회와 수동 검색/읽기
연결2회를 줄인 결과다. 학습 재사용은3→2회다. 단순 대화는 그대로1회이며 이 표에서
오히려0.40초 느렸다. 전체 채팅 개선율·통계적 p95·운영 TTFT를 주장하지 않는다.

공식 로그인은 기존 장치 안에서 사용했고 private bridge가 매 요청마다 공식 app-server를
초기화해 도구와 이력을 전달했다. 같은 bridge로 비교했지만 이 overhead는 운영 제품
성능이 아니다. baseline→candidate 고정 순서이며 warm/cold를 교차 반복하지 않았다.
v2는 새 사용자 namespace를 썼으므로 v1의 별도 SQLite·서버 기동 상태와 구분한다.

보고서의 저장된 `first_final_body_ms`와 `terminal_ms`는 누락이라 채우지 않았다.
위 시간은 harness의 접수→완료 관측이며 실제 Runtime model/tool spans도 별도로 보존했다.
A는 모델21794→9089ms, 도구16→12ms였다. 도구 반환 추론의 token 사용량은 unknown이다.
setup 실패는 task 시간에서 제외하되 보존했다. 첫 tool probe continuation에서 모델이
완료 대신 같은 verification tool을 다시 불러 HTTP502가 난 경우였으며 제품 DB의 probe
상태를 강제하거나 실패를 성공으로 표시하지 않았다. 입력·oracle은 변경하지 않았다.

## 추가로 발견한 분류 오류

깨끗한 기준960f9a5에서 기존 두 Runtime 검사의 실패를 재현했다. “저장된 문서를 읽어줘”를
새 보고서 저장으로 오해하던 판별은 저장된 입력 수식어와 현재 저장 요청을 구분하도록
고쳤다. 기존 Runtime 결과는 자료 읽기·답변4호출, 불필요한 mission 분류와 mutation0이다.
미래 읽기의 필수 이메일 전달을 모델이 task quote에서 버렸을 때 시간 질문으로 바꾸지 않고
지원 범위를 정확히 알린다. 직접 초안 준비·매 발송 최종 허락은 유지한다. “Gmail을 읽고
여기로 보내줘”는 기존 local 결과 전달 계약을 공유해 계속 지원한다.
이 두 보완은 위 실모델 snapshot 뒤에 추가했으며 별도 실제 Runtime/Linux 검사로 검증한다.

## 검증과 한계

Native 실제 Store·Runtime에서 새 child 연결, 재실행 비용 중복 없음, 다른 owner 거절,
구버전 profile, 모호/잘못된 자료, 복합 보고서 채택·새 입력 재사용·철회와 shared/personal
combined-tool protocol을 확인했다. 독립 검토에서 발견한 local 전달 회귀는 기존 공통
판별을 사용하도록 고치고 재검토했다. 일반 발송 허락·취소·철회·unknown dispatch·deadline
및 예산은 유지하며 운영 DB·기존 collector·cron·메일 발송·CI는 변경하지 않았다.

Linux 통합 첫 실행의 해당 경로들은 통과했고 넓은 Store mission 검사에서71개 통과,
3개는 예전 schema를 만들던 fixture 준비 단계에서 실패했다. 최신 native-mail 등의 후대
테이블/trigger 제거 순서를 기존 restore predecessor와 맞췄고 원래 rollback·권한 assertion을
유지했다. 이3개는 최종 Linux에서 각각 다시 통과했다. 처음 Clippy는 변경하지 않은 broker dependency의
기존 lint에서 멈췄으며 변경한 core/Store/agent의 scoped `--no-deps` 검사로 분리한다.
그 뒤 scoped Clippy에서 발견한 fixture helper의 인수 개수와 테스트 모듈 선언 순서는
테스트에 한정한 lint 지정·동일 함수 위치 이동으로 정리했다. 최종 Linux에서 두 기존
분류 실패, local 전달/실제 메일 전달 구분, 새 보고서 판별, 수정한3개 migration fixture가
모두 통과했다. 변경한 core/Store/agent의 feature 포함 Clippy `-D warnings`와 Linux CLI
빌드도 통과했다. 이후 동작 변경은 없고 문서만 갱신했다. 전체 release 수락이나 모든
provider의 품질 증거가 아니다.

## 재현 식별자

- 기준 source: `960f9a5cc5bcf56fb9bfea2ffd0051e494ab2ba2`.
- 기준 binary SHA256: `48c525e5e3eb41b09e5a4ca7e95505c2a97b6cfe7ef91b08f8b0ac26c21ca40c`.
- 비교 candidate binary SHA256: `b760354381a0861708340bb08b5d983b62e853bfe65a3476794ba0a06867a8e2`.
- 고정 inputs SHA256: `376c1b3247c530482c0edf4e716d991a955d1217d7342bfea3b996ddc4eadad7`.
- 비공개 oracle SHA256: `b06797aabe61598adee0063ece86730924731c7834fd68d8a4480f9b8a9947e0`.

원시 trace·DB·credential·oracle 본문은 공개하지 않는다. 이후 실행 환경을 바꾼 재평가와
초기 실패는 각 원래 기록을 유지하며 이 표에 조용히 합치지 않는다.
