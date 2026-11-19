# 현재 상태

2026-11-19 기준. 운영 Linux는 **회원마다 컨테이너 하나를 쓰는 Kiri 0.2.1**로 서비스한다. 공개 한 줄 설치기는 **표준 서버 개발판**을 설치한다. 표준 서버에서 검증한 기능이라도 Kiri의 회원 화면에서 쓸 수 있는지는 아래 운영 범위를 확인한다. [설치판 안내](install.md) · [Kiri 운영 안내](../deployment/kiri/README.md)

## 운영 중인 Kiri

| 영역 | 확인한 동작 | 남은 일 |
| --- | --- | --- |
| 계정과 개인 데이터 | 로그인·대화·수동 기억·Google 권한은 회원 볼륨에 저장한다. 설치 관리자는 회원을 초대·정지하고 사용량을 확인한다. [관리 안내](../deployment/kiri/admin.md) | 현재 운영은 새 볼륨으로 시작했다. 기존 데이터와 키를 보존하는 전환 및 독립 복구는 일부 합성 자료로만 검증했다. [전환 작업표](../openspec/changes/isolate-account-runtime/tasks.md) |
| 모델 | 설치 관리자만 모델과 추론 강도를 지정한다. 운영 회원 소켓에서 Claude Sonnet 5.5 / medium으로 산술 응답, 다음 문서 선택, 시간표의 출발 시각과 승강장 답변을 확인했다. 게이트웨이·모델 브로커·회원 컨테이너에서 같은 배포본을 확인했다. [엔진 설정](../deployment/kiri/admin.md) | 운영 ChatGPT 연결에는 별도 인증이 필요하다. [구독 연결](../deployment/kiri/subscriptions.md) |
| 작업 브라우저 | 대화에서 공개 페이지 읽기를 맡기고 같은 탭을 보거나 직접 조작한 후 작업을 재개할 수 있다. Linux Chromium에서 회원 간 화면 격리를 확인했다. [사용 안내](../deployment/kiri/browser.md) | 실물 iOS·Android 재진입과 여러 회원이 동시에 사용할 때의 지연 측정이 남아 있다. |
| Google Workspace | 연결 화면에 Gmail·Drive·Docs·Slides·Sheets의 계정과 상태가 표시된다. 격리한 Linux에서 동의 시작과 합성 Gmail 조회를 확인했다. [회원별 연결 절차](../deployment/kiri/google.md) | 운영 Google OAuth 클라이언트 등록과 첫 실계정 동의가 남아 있다. |
| 볼트 | 독립 볼트 웹에서 로그인 항목을 관리할 수 있고 Kiri 회원 화면에서 연결 상태를 확인한다. [볼트 웹](vault-service-client.md) | Kiri 로그인에서 볼트 세션으로 이어지는 인증과 에이전트의 일반 로그인 항목 사용이 남아 있다. |

## 표준 서버와 출시 준비

표준 서버 개발판은 저장되는 대화·개인 자료 조회·자동 기억 추출·예약 업무·후속 모니터링을 지원한다. 같은 공급자 연결에서 관리자 모델이나 추론 강도가 바뀌면 다음 호출에 반영한다. 이미 공급자에 보낸 요청은 원래 모델로 정산한다. [모델 설정](provider-settings.md) · [자동 기억](automatic-memory.md) · [예약 업무](missions.md) · [모니터링](task-monitors.md)

- **설치와 접속** — macOS·Linux·WSL2용 설치 흐름과 한국어·영어 안내가 있다. Tailscale과 도메인 HTTPS 설정, Telegram 연결은 각 안내에서 지원 범위를 확인한다. [설치](install.md) · [Tailscale](tailscale.md) · [외부 접속](public-access.md) · [Telegram](telegram.md)
- **비서 기능과 품질** — 기능별 동작과 제한은 기능표에, 평가 방법은 시험 계획에 기록한다. 새로운 시나리오를 이용한 독립 품질 평가와 실기기 성능 측정이 남아 있다. [기능표](features.md) · [평가 방법](agent-evaluation.md)
- **안정판 출시** — 볼트와 자격 증명 도우미의 공식 패키지 배포, 운영 환경에서 다른 호스트로의 복구 검증, 플랫폼별 실기기 수락이 남아 있다. [출시 조건](public-release.md) · [구성요소 패키지](component-packages.md) · [전체 계획](plan.md)

## 검증 기록을 읽는 기준

운영에서 확인한 결과, 합성 공급자로 통과한 시험, 실기기에서 남은 검증을 구분한다. 과거 기록의 모델명·스키마·기능 제한은 각 시험 시점에 해당한다. [시험 기준](testing.md)

이전 배포 순서, 기능별 검증표와 당시 미완료 항목은 날짜별로 보존한다. [11월 19일까지의 누적 기록](reviews/2026-11-19-status-history.md) · [11월 13일까지의 누적 기록](reviews/2026-11-13-status-history.md)
