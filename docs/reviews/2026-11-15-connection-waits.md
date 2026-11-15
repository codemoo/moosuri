# 계정 선택과 같은 업무 이어하기 검증

2026-11-15, `1772b31` 이후 B 단위. 실제 provider 계정 대신 합성 OAuth 연결·임시 SQLite와 격리 Linux를 사용했다.

처음 지원/probe한 Gmail 읽기 slot만 채우며 원래 도구 profile을 바꾸지 않는다. 유효한 단일 연결은 재사용하고 여러 계정은 원래 업무 카드에서 한 번 선택한다. 명시한 계정이 없으면 다른 계정으로 실행하지 않는다. 연결된 서비스의 scope/device/epoch 변경과 모호한 dispatch는 재개 권한이 아니다.

독립 검토에서 복합 Gmail/Calendar 재개 시 중복 profile INSERT, 이전 wait의 상태로 새 ready가 가려지는 문제, 대기 취소 버튼의 early return, 명시한 없는 계정의 암묵적 fallback을 발견했다. 정확한 profile 재사용·wait UUID·Cancel 분기·계정 표현 검사로 수정했다. 후속 검토가 발견한 “계정으로/계정은” 한국어 조사도 같은 검사에 포함하고 송신인 from: 필터와 구별했다. 수정 후 독립 소스 검토에서 남은 P1/P2는 없다.

| 검증 | 실제 결과 |
| --- | --- |
| Linux Store `connected_mission_` | 8 통과: 실제 읽기 후 같은 run/step 재개, Gmail+Calendar, 계정 선택/중복 요청, 2 waiting+1 paused 용량·oldest ready, 만료·철회·원래 예산/권한 |
| 한국어 명시 계정 후속 | 1 통과: 없는 B를 요청하고 A만 연결됐을 때 fallback 없음, sender filter는 별도 |
| Linux predecessor/current restore | 각각1 통과, migration60·기존 삭제 journal·wait/slot 격리 |
| React 맡긴 일 | 19 통과, 계정 선택 ACK 유실의 같은 UUID 재확인·대기 취소 포함 |
| TypeScript·Linux server Clippy | 통과 |

첫 Linux 검사 명령은 잘못된 module filter 때문에0개를 실행했다. 이를 결과로 세지 않고 `connected_mission_`로 바꿔 실제8개 결과를 확인했다. 최종 한국어 계정 회귀 source archive SHA256은 `5c3bcccb6036313bcde89ea2663b720332e94aa8021bb6b4bcace78b41911f1f`이며 Linux 검증 중 source hash는 일치했다.

ordinary chat120초와 mission 원래 최대하루를 구별하고, 대기·unlock·refresh는6model/12tool/2repair 한도를 초기화하지 않는다. ready가 잠시 실행할 자리를 얻지 못해도 DB에 보존하고 원래 기한에 만료시킨다. 별도 native Gmail의 실제 기기 도구·기기 응답 재개는 C 단위이며 여기서 완료로 주장하지 않는다. 실계정·실기기·운영 수락·CI 활성화는 수행하지 않았다.
