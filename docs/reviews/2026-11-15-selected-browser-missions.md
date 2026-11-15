# 선택 페이지와 원래 업무 연결 검증

2026-11-15, A 이후 첫 F 단위. 합성 Store·CDP loopback·React와 Linux 컴파일로 확인했다. 실제 사용자 사이트 로그인·쿠키·암호·메일 발송은 사용하지 않았다.

Linux `selected_browser_page_`에서 실제 저장 source를 원래 미션이 읽고 실제 보고서로 저장하는 경로, ACK 유실 재확인, 원래 run/profile/호출 수/기한 보존, 다른 계정·조작권·미션 revision·오래된 관측 차단을 확인했다. 원래 대화 철회는 복사한 source와 보고서를 모두 철회한다. 추가한 restore quarantine helper의 동일 철회 회귀1개도 Linux에서 통과했다.

CDP loopback은 화면 이후 document loader가 바뀌면 관측을 차단하고, 현재 frame을 선택한 한 번만 새 관측을 허용하며 만료/재사용을 거부한다. 입력/탐색 dispatch는0이다. 새 worker operation을 포함한 모든 connector examples가 컴파일됐고 server Clippy가 통과했다. React15개 검사와 TypeScript는 선택 카드의 원래 업무 연결·응답 유실의 같은 요청 재확인을 포함한다.

독립 소스 검토는 owner/session/control/frame/mission identity, generation 전진, 저장/복원/철회·재시도 경계에서 material P1/P2를 발견하지 않았다. Linux 최종 기능 소스 archive SHA256은 `5db15551509f2952a5cd86e5333070e4c7ad74e246ca229d046576aa3526dfa2`; 검증 중 해시가 일치했다. 이후 restore helper 전용 회귀도 통과했다. 해당 archive SHA256은 `5333fecc24d2352a90e3cb52034d772c86a7f8c456a0ec101fcce0346f26699b`다.

이 증거는 실제 Chromium 캡슐 배포·실제 사이트/기기 수락을 대신하지 않는다. 새 worker가 없는 배포에는 자동 fallback하지 않는다. 신뢰한 기기의 browser/broker·vault password/TOTP·사이트별 adapter는 이 단위에 포함되지 않는다. schema66은 앞선 통합 migration 뒤에 적용해야 한다.
