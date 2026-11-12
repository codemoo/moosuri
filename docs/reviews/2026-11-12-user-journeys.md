# 사용자 여정 기반 계획 검토

2026-11-12. 기존 계획을 상태 전이와 반례로 읽은 기록이다. 아래 `계약 확인`은 문서에서 구현 가능한 규칙을 찾았다는 뜻이며 runtime 테스트 통과가 아니다. 현재 구현/검증 상태는 [status](../status.md)를 따른다.

| 여정·반례 | 확인한 동작 계약 | 판단·근거 |
| --- | --- | --- |
| 빈 서버, 모델 연결 생략·임시 admin 비밀번호 | 관리 화면 시작·admin 하나·변경 전 제한 세션·일회 TTY/UI 표시 | 계약 확인: [onboarding](../onboarding.md), SEC02 |
| 사용자 A 로그아웃 뒤 B 로그인, A의 늦은 응답 | tenant/session generation·cache/Worker/Push 수명으로 B에게 전달하지 않음 | 계약 확인: [isolation](../accounts-isolation.md), SEC01/UX04 |
| 사진 전송 실패 후 새 글을 쓰다가 재시도 | 같은 요청/첨부 ID로 재개·새 composer 초안 보존 | 계약 확인: [features](../features.md), CHAT04/UX02 |
| 기억 정정·망각 후 restart/이전 checkpoint | revision·provenance·망각 tombstone·최신 journal과 삭제 범위 적용 | 계약 확인: [architecture](../architecture.md), MEM04/MEM05 |
| community 추가·연결 화면 전환 뒤 원래 작업 복귀 | 영속 run·사용자 binding·고정 package revision·대기/재개. 설치만으로 외부 실행 없음 | 기존 계약으로 구현 가능: [community](../community-skills.md), COMM06/CRED04. 별도 continuation 서비스 추가 근거 없음 |
| 기본 모델이 media를 지원하지 않음 | 실제 capability와 선택 media 연결로 지원 상태 표시·몰래 cloud fallback 금지 | 계약 확인: [providers](../providers.md), PROV01/PROV03/UX03 |
| DST fold·서버 중단·claim 뒤 예약 수정 | 첫 offset1회·latest-only misfire·revision/dispatch commitment 순서 | 계약 확인: architecture, AUTO01/AUTO02 |
| 이미 연결한 credential 재사용 중 revoke 경합 | 기기 local grant·online commitment 선후·기기 ACK pending·in-flight 정산 | 계약 확인: [credential-use](../credential-use.md), CRED01/CRED03 |
| 기기 A 발송 후 ACK 유실, B가 새로 연결됨 | 같은 effect owner/journal로 reconcile 또는 unknown·자동 failover 재발송 없음 | 계약 확인: credential-use, CRED05 |
| host/config/secret 전부 상실, 암호화 backup만 남음 | 기존에는 outer key 출처가 없어 복원이 불가능할 수 있었음 | 보완: 운영자 offline identity·key epoch 보관·최신 journal/검증 자료와 독립 target 접근. OPS03/VAULT05/OpenSpec에 반영 |
| 과거 snapshot 복원 뒤 계정/provider 재등록 | 옛 권한/사용량 부활 차단·미완료 작업 quarantine 유지·새 지출은 별도 reconcile/예산 | 계약 확인: architecture, SEC04. 재등록을 과거 작업 재실행 허가로 해석하지 않음 |
| 남의 learned skill 설치·schema 변경 | 배포자 proof 승계 없음·개인 candidate·actual finalized 증거·version 무효화 | 계약 확인: community, SKILL02/SKILL04/COMM02 |

독립 검토는 제품 여정·복구/격리·출시/평가의 세 범위로 나누었다. 제품 여정과 출시/평가에서는 새 중대 지적이 없었고, 복구에서 outer-key 보관 누락1건을 확인했다. 일반 run/계정/격리 계약으로 이미 처리 가능한 흐름을 이유로 새 서비스·테스트 개수 목표·반복 승인 단계를 도입하지 않았다.

복구 키 수정본의 좁은 독립 재검토에서 남은 중대 결함 없이 문서 계약상 해결을 확인했다. 완료 증거는 문서/시나리오/OpenSpec 일관성 검증과 이 재검토이며, 실제 clean-host 복원·개인 key 생성·provider 호출·제품 배포는 이번 검토에서 수행하지 않았다. 기존 OPS03의 drain/takeover·fsync/lock·orphan child 검증도 유지한다.
