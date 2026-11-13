# Google 설치 설정 마법사 수락 기록

범위: R04/R09/R10, F01/F34/F56. 독립 셀프호스팅 설치자가 Google OAuth 웹 앱 하나를
등록하고 각 회원이 자신의 Google 계정을 연결한다. 중앙 OAuth 중계는 운영하지 않는다.

## 구현

- `/admin/google`: Google Cloud 준비 → 신뢰한 origin의 콜백 복사 → 최대16KiB 웹
  클라이언트 JSON 확인·명시적 저장 → 본인 Google 로그인·프로필 확인.
- SQLite migration10: 설치 단위 암호화 설정·CAS revision, OAuth client identity와
  app revision, 원래의 setup return intent. secret은 기존 설치 키와 domain-separated
  AAD로 암호화한다. 서버 관리 connector credential이며 개인 키 볼트가 아니다.
- 명시한 운영자 파일은 우선하고 웹 저장을 거절한다. 저장 설정은 바로 적용되고
  재시작 후 유지된다. Google endpoint는 고정되며 입력 JSON의 endpoint를 쓰지 않는다.
- 모든 tenant의 미완료 OAuth를 검사하고 claimed exchange도 완료/취소/만료 전에는
  설정 변경을 막는다. 다른 client/callback으로 기존 개인 grant를 인수하지 않는다.
- 같은 client의 secret 회전은 refresh 성공·오류의 app revision을 검사한다. 진행 중
  Gmail 수집 run은 unknown으로 전환해 늦은 결과를 막고 기존 자료/checkpoint를 보존한다.
- 연결 확인은 요청한 관리자의 개인 profile만 읽는다. 메일 조회/발송·수집 등록은 없고
  기존 메일 발송의 전체 미리보기와 명시적 최종 승인 계약을 유지한다.

## 검증

- macOS Rust workspace **332개 통과**, 실모델 opt-in1개 제외. 새 핵심 검증은
  Store8개(암호화/재시작/변조, admin·제한 세션, CAS, 전역 교체 제한, OAuth 각 단계,
  stale refresh 성공/실패, source lease)·HTTP3개·parser2개다. 내장 경로 시험에도
  `/admin/google`을 추가했다. 기존 mail callback 시험은 실제 runtime의 app pin을 사용한다.
- React **128개 통과**, TypeScript·production build·생성 DTO 일치·Rust fmt와
  workspace/all-target clippy `-D warnings` 통과. 불필요한 기존 테스트 이식은 없다.
- 실제 Rust 서버·합성 계정으로 Chromium에서 로그인→콜백 복사→파일 확인→저장→
  개인 Gmail 활성화→서버 종료/재시작 후 설정·로그인 유지를 확인했다. 저장 후 입력 파일
  상태가 지워지고 canary가 DOM·local/session storage·저장 파일에 평문으로 남지 않았다.
  1280×900/390×844를 확인했으며 모바일 문서 폭390px, 내부 스크롤과 마지막 연결
  버튼 접근을 검증했다. 실제 iOS/Android 시험은 아니다. 시험 서버·브라우저를 닫고
  합성 비밀번호·JSON·DB를 삭제했다.
- Linux x86_64/Rust1.89의 고정 Docker 이미지에서 offline/locked workspace build와
  **332개 테스트 통과**, 실모델 opt-in1개 제외. non-root·read-only source/rootfs·network
  none으로 실행했고 최종 소스/웹 자산 SHA256이 시험 snapshot과 일치함을 확인했다.
- Linux 실제 CLI/HTTP 여정 **8/8**: 설치·내장 관리자 경로·정확한 콜백·5개 관리자 API의
  회원 차단·검증과 저장 구분·CAS·즉시 활성화와 개인 연결 분리·재시작 보존을 확인했다.
  Google/모델 호출0회, 서버 로그 비밀값 노출 없음, 합성 credential/DB 삭제와 서버 종료,
  시험 컨테이너0개를 확인했다. 앞선 두 번의 시험 패키지 누락은 harness에서 수정한 뒤
  완전한 snapshot으로 다시 검증했다. 제품 결함이나 테스트 성공으로 집계하지 않았다.

## 독립 검토와 수정

Sol/high 독립 검토자는 최종 통합 source의 암호화/CAS·origin·operator override·전역
OAuth 보호·refresh 성공/오류·source lease·본인 확인·React 계정/콜백 경계를 읽고
blocking finding 없음으로 보고했다. 검토자는 코드·테스트를 읽었으며 실행 시험이나
실제 Google 연결 검증을 수행했다고 표현하지 않는다.

주 통합 과정에서 저장된 설정의 콜백 불일치가 `configured=false`여서 안내가 숨겨지는
화면 결함을 수정했다. 불완전한 동의/프로필·토큰 교환 실패는 claimed flow를 취소해
설정 수정을 불필요하게 막지 않는다. 프로필 확인 중 app revision이 바뀌면 성공으로
표시하지 않는다. UI 계약은 중복 선언 대신 Rust 생성 DTO를 사용한다.

## 범위와 한계

파일 확인은 JSON 형식·정확한 콜백 일치만 증명한다. 실제 Google 프로젝트 생성,
Gmail API 활성화, 동의 화면·테스트 사용자·앱 검증은 설치자의 작업이다. 이 단위에서
Google/메일/LLM 실계정 호출과 운영 배포는 하지 않았다. 실계정 OAuth·프로필 확인은
설치 후 남아 있으며 synthetic 통과를 그 성공으로 대체하지 않는다.

경쟁 시험은 주로 Store의 pinned carrier를 제어한다. MailService 네트워크 refresh를
멈춘 상태에서 rotation시키는 통합 시험과 성공/불완전 동의/프로필 오류의 HTTP 전체
여정은 별도 coverage 한계다. 백업·키 유실 복구·일반 배포 패키지는 이 단위의 범위가 아니다.

Cargo.lock·웹 lock·라이선스와 의존성 목록은 이전 검증에서 바뀌지 않았다. 공개 반영 전
OpenSpec23개·계획 메타데이터/링크·파일 허용 목록·비밀 탐지를 통과했다.
GitHub Actions 비활성을 확인했으며 그대로 유지한다.
설치 방법과 공식 Google 문서는 [메일 안내](../mail.md)에 연결했다.
