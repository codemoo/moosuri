# Gitea 실제 로그인·조회와 원래 채팅 수락

2026-11-15. 첫 사이트 adapter를 공개 소스가 있는 Gitea1.24.6으로 고정했다.
기존 계정 대신 격리된 실제 서비스·비관리자 합성 계정·TOTP·비공개 저장소를 사용한다.
다른 사이트, 임의 폼/브라우저 미션, 실제 사용자 계정·물리 기기 수락이나 운영 배포를
뜻하지 않는다. [지원 범위와 원본 근거](../native-credential-gitea.md).

## 사용자 결과

신뢰한 기기에서 독립 암호화 볼트의 정확한 항목/origin/저장소를 한 번 연결한다.
원래 채팅은 서명된 native 결과를 기다리며, device-local Chromium이 비밀번호와 현재
TOTP를 입력한 후 해당 저장소의 open issue 개수만 반환한다. 같은 범위의 두 번째
요청은 binding과 로그인 세션을 재사용한다. 서버·모델은 암호/TOTP/code/cookie/DOM/CDP를
받지 않으며 일반 브라우저·쓰기 권한은 생기지 않는다.

기존 protocol wire 값은 유지하고 새 값9/10/24만 추가했다. DB schema를 추가하지 않으며
기존 live/archive vault 검사·원래 대기/종료·수명·철회 경계를 유지한다.

## 검증 구성

실제 서비스 image는 Gitea1.24.6의 고정 digest이며 container는 nonroot, cap-drop,
read-only, network none, 외부 host port/egress 없음이다. 시험 Chromium은 같은 network
namespace에서만 loopback 서비스에 접근했다. 시험 전용 headless/no-sandbox wrapper와
orphan reaping용 init을 사용했으며 제품의 headed 실행 flags는 바꾸지 않았다.
운영 서버·기존 서비스·cron·실제 계정은 변경하지 않았다.

통합 React credential4개, production build/typecheck를 통과했다. Linux에서 실제 Store
시험 컴파일과 local_browser lifecycle7개를 통과했다(별도 opt-in 시험1개는 기본 실행에서 제외). navigation context loss와 graceful close를 포함하며, `gitea` 이름 필터는 매칭0개라 별도 policy 시험 통과로 계산하지 않는다.
같은 원래 채팅을 완료하고 새 채팅에서 같은 session을 재사용하는 실제 경로와
서버 SQLite/WAL·native journal의 password/TOTP canary 검사를 최종 수락 항목으로 삼았다.

## 실제 실패에서 수정한 것

- Strict sign-in Gitea는 API에서 browser session을 받지 않았다. upstream에서 AppVer로
  만드는 정확한 asset version을 로그인 전과 인증된 repository 화면에서 각각 검사한다.
- TOTP 제출 후 문서 교체로 사라진 CDP context는 read-only 상태 관측만 제한적으로
  다시 읽는다. 비밀번호/TOTP POST를 재제출하지 않는다.
- 브라우저 종료는 graceful 종료·owned process reap·전체 group 소멸을 확인하고 profile을
  정리한다. graceful await 취소에도 RAII가 아직 소유한 group을 종료한다. reap 뒤에는
  저장된 PID로 신호를 보내지 않고, 정리 불명은 Unknown과 profile 보존으로 남긴다.
- 초기 시험 container의 PID1/orphan 정리 경계와 종료 직후 group 소멸 지연을 분리해
  실제 init 환경과 제한된 읽기 관측으로 검증했다. group-empty 검사는 약화하지 않았다.

원본 기능과 version/navigation 수정을 독립 검토했고, 종료 취소의 P2를 찾아 수정했다.
최종 lifecycle 독립 검토와 실제 통합 실행 결과는 아래에 기록한다.

## 최종 관측

통합 archive SHA256 `7a3ed8c40405c839d73f5ee31be9cb0d6ba2b6622e79cedcaf2473bc59dde0eb`의 Linux source hash를 확인했다.
동일 통합 Store binary로 실제 Gitea password/TOTP 로그인→issue3개→원래 채팅 완료→
같은 session의 새 요청 완료→정상 종료→SQLite/WAL/native journal canary 검사까지
단일 실제 시험을 통과했다. 첫 요청에서만 비밀번호/TOTP가 제출되고 두 번째는 기존
세션을 사용한 고정 stage 관측도 확인했다. 약5.8초는 전체 합성 시험 관측 시간이며
사용자의 일반 대화 지연이나 실제 기기 성능 수치가 아니다.

최종 `db265b7` lifecycle 독립 재검토에서 취소 P2가 해결됐고 새 중요한 P1/P2는
발견되지 않았다. 공식 서명 artifact·physical headed device·다른 Gitea 버전과
타 사이트/실계정 수락은 별도다. CI는 계속 꺼져 있다.
