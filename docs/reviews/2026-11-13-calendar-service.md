# Calendar 공통 실행부 검증

공개 기준 `a6ab50d`의 직접 양식 Calendar 실행을 `moosuri-agent::calendar::CalendarService`로
분리했다. HTTP 인증·OAuth 경로는 유지하고, 연결/refresh 확인·목록/일정 조회·단일 dispatch·
최소 영수증·동일 ID의 조회 복구를 같은 실행부에서 처리한다. schema·의존성·외부 목적지·
사용자 승인 방식은 바꾸지 않았으며 agent/대화/미션 Calendar 권한을 추가하지 않았다.

독립 검토는 739개 소스의 해시를 확인하고, 변경한 4개 코드 경로의 소유권·ETag·원래 요청
identity·중복·늦은 결과·호출 취소 경계를 검토했다. 도입된 중대 결함은 발견하지 않았다.
이후 root는 동일 코드 해시를 통합하고 Linux의 격리된 network-none 컨테이너에서 검증했다.
원래 source manifest SHA256은
`023da7799037248890aa4304e873db721ad7a605bbf050692dc870e72508037c`다.

| 검증 | 결과와 범위 |
| --- | --- |
| Service + 실제 합성 Store | 8개 통과: 동일 요청/변경 payload, 불명 결과의 원래 operation 확인, delete 현재 부재, dispatch 전 철회, dispatch 후 logout/disconnect, ETag 충돌, 부분/늦은 읽기 |
| 기존 Store Calendar | 15개 통과: 계정·동의·권한 세대·쓰기 ledger·복구 계약 |
| 기존 connector Calendar | 5개 통과: 실제 loopback 전송·제약·응답 처리 |
| HTTP Calendar | 1개 통과: 별도 scope·실제 state·소유자별 재시도 영수증 |
| 정적 검사 | 변경 코드 rustfmt, agent/server all-target Clippy `-D warnings` 통과 |
| 문서/계약 | 계획 메타데이터와 OpenSpec strict 검증 통과; 제품 성능 시험으로 계산하지 않음 |

Linux Clippy의 최초 실행은 Cargo 하위 명령에 vendor 설정이 전달되지 않아 offline `hkdf`
해결에 실패했다. 설정을 `clippy` 뒤에 전달한 재실행은 통과했다. 제품 소스 수정으로 오류를
숨기지 않았으며 최초 실패와 수정 실행 결과를 별도로 보존했다. native와 Linux의 전체
workspace 시험을 새로 수행했다고 주장하지 않는다.

Service fixture는 횟수와 지연을 제어한 typed IO이고 connector 검사는 실제 loopback이다.
Google 실계정 호출·실제 일정 변경·새 자연어 이해·새 UI·실기기·운영 배포 검증은 아니다.
명시적 호출 취소의 기존 동작은 소스로 검토했으며 새로운 동적 취소 시험으로 세지 않는다.
대화 읽기/쓰기의 실행 권한·자료 철회·시간/대상 해석 연결은 별도 남은 작업이다.
