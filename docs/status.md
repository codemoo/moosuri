# 현재 상태

2026-11-17. 실제 지원 범위·검증 한계·남은 작업은 이 문서가 정본이다.

[Telegram 개인 대화](telegram.md)를 PWA와 함께 사용할 수 있다. 관리자 봇·계정별
일회용 연결로 같은 기억·도구·저장 대화를 사용하며, 추가 지침·맡긴 작업 중지·웹 재시도와
선택적인 소식함 링크 알림을 연결했다. 공개 웹훅이나 도메인은 필요 없다.
독립 검토·합성 Bot API/모델 통합·계정/전송/철회·기존 대화/스키마/복원·React 검사와
Mac/Linux lint 및 네 플랫폼 빌드를 통과했다. Linux x64·ARM64 QEMU 실행도 확인했다.
[v0.1.0-dev.8](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.8)의 네 플랫폼
서명 패키지와 한 줄 설치로 공개했다. 실제 Telegram 계정/기기 수락은 별도다.
개인 텍스트를 지원하며 파일·음성과 메일 최종 발송 승인은 웹에서 처리한다.
[검증 기록](reviews/2026-11-17-telegram.md).

[공개 셀프호스팅](public-access.md)의 등록된 DNS·일회용 HTTPS 응답 검증·내장 인증서 갱신,
기존 프록시 연결·상태·중단 복구·이전 주소 복원을 구현했다. Tailscale과 공개 접속을
선택하며 로컬 연결은 나중에 설정할 수 있다. DNS 기록은 생성·수정하지 않는다.
독립 검토·Mac/Linux 설치 계약·양쪽 전체12개 터미널 시나리오·lint와 네 플랫폼
빌드를 통과했다. [v0.1.0-dev.7](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.7)의
네 플랫폼 서명 패키지와 한 줄 설치로 공개했다. Linux x64·ARM64 QEMU의 웹·로그인·
종료와 공개 한 줄 명령의 dev.7 다운로드·한국어 설치·임시 암호 강제 변경·웹 로그인·
정상 종료·데이터 보존 제거를 확인했다. 실제 DNS/CA 발급과 외부 기기 접속은
별도 수락 범위다.
[검증 기록](reviews/2026-11-17-public-access.md).

[Tailscale 연동](tailscale.md)의 주소 자동 탐지·기존 프로필 로그인·전용 HTTPS 설정,
connect/status/disconnect, 소유 경로 보존·중단 복구와 이전 주소 복원을 구현했다.
독립 검토·native/Linux 설치 계약과 Tailscale 중단 복구·충돌 보존·로그인 검증,
전체 Linux 터미널 시나리오와 lint를 통과했다.
[v0.1.0-dev.6](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.6)의 네 플랫폼
서명 패키지와 한 줄 설치로 공개했다. Linux x64·ARM64 QEMU의 웹·로그인·종료도
확인했다. 실제 Tailscale 로그인·인증서·기기 간 접속은 별도 수락 범위다.
공개 한 줄 설치의 dev.6 다운로드·한국어 설치·임시 암호 강제 변경·웹 로그인·
정상 종료·데이터 보존 제거도 격리된 Mac에서 통과했다.
[검증 기록](reviews/2026-11-17-tailscale-connection.md).

[설치·일상 제어 후속 보완](reviews/2026-11-17-daily-controls.md): `open --start` 및 메뉴의 시작하고 열기, 자동 시작 설정을
유지하는 `restart`, 다른 설치의 포트 점유 감지, 서비스 파일 충돌 시 터미널 설치
완료, WSL 바로가기 갱신을 구현했다. 독립 검토·native/Linux 설치 계약과 실제
터미널 시나리오·lint, Mac의 임시 launchd 시작/열기/재시작/제거를 통과했다.
[v0.1.0-dev.5](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.5)의
네 플랫폼 서명 패키지와 한 줄 설치로 공개했다. Linux x64·ARM64 QEMU의
초기화·웹·로그인·종료를 확인했다. 실제 공개 한 줄 명령의 dev.5 다운로드·한국어
설치·임시 암호 강제 변경·설치 식별이 포함된 준비 확인·데이터 보존 제거도 통과했다.

[기능표](features.md)는 요구 계약, [계획](plan.md)·[이전 순서](migration.md)는
전체 단계, 아래 날짜별 기록은 해당 소스의 증거다. 공개 개발 저장소는
[codemoo/moosuri](https://github.com/codemoo/moosuri)이며 안정판·운영 전환은 아직 아니다.
GitHub Actions/자동 CI는 사용자 지시로 꺼져 있다. 로컬 검증을 작업 단위별로 수행한다.

[설치 복구·실행 관리](reviews/2026-11-17-installation-reliability.md)를 보완했다.
완성된 실행 파일과 선택한 포트를 한 번에 적용하고, 설치 중단 후 재개·동시 제어·
실행 중 foreground 서버 인식·웹 준비 후 브라우저 열기를 연결했다. 준비 확인은
전체 20초로 제한하며 트레이에는 실행 가능한 웹 항목과 실패별 복구 안내를 제공한다.
메뉴는 설치 터미널 종료에도 유지되고, 중지·제거는 실제 실행 잠금 해제까지 확인한다.
독립 검토와 native/Linux 계약·양 언어 PTY·lint, 네 플랫폼 빌드 및 실제 임시
launchd 설치/제어/제거를 통과했다. [v0.1.0-dev.4](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.4)
네 플랫폼 서명 패키지와 한 줄 설치로 공개했다. Linux x64 실행 및 ARM64 QEMU의
초기화·로그인·종료도 확인했다. 실제 공개 한 줄 명령으로 dev.4 다운로드, 한국어
방향키/번호 설치, 임시 암호 강제 변경, 웹 접속과 데이터 보존 제거까지 통과했다.

[데스크톱 메뉴](desktop-menu.md)를 추가했다. Mac 메뉴 막대·Linux 트레이에서 서버 상태,
시작/중지, 로그인 자동 시작, 채팅/웹 관리자 바로가기를 제공한다. 메뉴 종료는 서버를
멈추지 않으며, 서버 중지는 모든 사용자·예약에 적용된다. headless는 CLI/서비스,
WSL2는 웹 바로가기를 유지한다. 독립 검토와 native/Linux 계약·양 언어 PTY·lint·
네 플랫폼 빌드, Mac의 실제 임시 launchd 시작/중지·자동 시작·제거를 확인했다.
메뉴 화면 도구가 시간 초과하여 실제 클릭/시각 검증은 남으며 Linux 패널/systemd·
Intel/WSL2/물리 ARM 수락도 별도다. [검증 기록](reviews/2026-11-17-desktop-controls.md).
[v0.1.0-dev.3](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.3)의 네 플랫폼 서명 패키지와 한 줄 설치로 공개했다.
실제 공개 명령의 dev.3 다운로드·설치·로그인/임시 암호 강제 변경·종료·데이터 보존 제거까지 확인했다.

[한국어·영어 설치 마법사](install.md)를 구현했다. 방향키/번호 선택, 최초 관리자,
선택적 API/local 모델 확인, 로컬 접속, 사용자 launchd/systemd 및 foreground 실행을
기존 서명 설치에 연결했다. 합성 PTY의 양 언어 설치·재실행과 독립 검토를 통과했다.
Mac·Linux x64/ARM64의 [서명된 설치 미리보기 v0.1.0-dev.2](https://github.com/codemoo/moosuri/releases/tag/v0.1.0-dev.2)와
한 줄 bootstrap을 공개했다. Mac ARM64·Linux x64 실행과 Linux ARM64 QEMU 실행을 확인했다.
Intel Mac은 교차 빌드이며 실제 Intel·WSL2·물리 ARM 및 사용자 서비스 실운영 수락은 남는다.
Linux/WSL2는 glibc2.34 이상이 필요하다. [설치 검증 기록](reviews/2026-11-17-guided-installation.md).
실제 공개 명령의 다운로드·한국어 방향키 설치·임시 암호 강제 변경·웹/API 접속·정상 종료·
데이터 해시를 보존한 제거를 격리된 Mac ARM64 환경에서 확인했다.

현재는 [기능별 안정화](testing.md#기능별-안정화)에 집중한다.
[메일 답장 감시](mail-reply-watches.md)를 추가했다. 완료된 Gmail 읽기/발송 영수증에서
해당 대화만 확인하고, 새 수신 메일이 없으면 모델·본문 조회 없이 넘어간다. 새 답장은
원래 대화의 실제 본문 미리보기와 소식함으로 전달하고 감시를 완료한다. 대화에서
등록·상태·중지를 지원한다. 재등록·과거 메일 조회의 중복 알림도 고쳤다. 독립 검토·
native/Linux 관련29계약·웹45검사·기존 합성15흐름/62판정·lint/제품 빌드를 통과하고
공개했다. [검증 기록](reviews/2026-11-17-mail-reply-watches.md). 자동 의미 요약·native 기기 메일 감시·운영 배포는 포함하지 않는다.
[추가 지침·답변 완료 처리](reviews/2026-11-16-answer-settlement.md)를 하나의
트랜잭션으로 묶었다. 완료 저장 실패 뒤 지침만 반영됨으로 남거나, 이전 답변의 표시
변경에 실패해도 다음 응답을 시작하던 결함을 재현하고 고쳤다. 실행 종료 처리는 한
Store 모듈로 모으고, 업무의 근거 검증과 일반 대화 완료는 구분한다. 수정 전 2실패와
수정 후 rollback·취소·추가 지침 이어하기를 확인했고, 독립 검토·native/Linux 관련
42검사·합성 15흐름/62판정·lint 및 공개 소스 Linux 빌드를 통과하고 공개했다.
운영 배포는 별도다.
[모델 응답 저장](reviews/2026-11-16-round-checkpoints.md)의 중복 구현 4곳을
공통화했다. 저장 대기열이 잠깐 찼을 때 맡긴 업무만 응답을 포기하던 차이를 재현하고,
일반 대화·분류기·Claude와 같은 제한적 로컬 재시도를 적용했다. 취소·권한 변경은
매번 Store에서 확인하며 모델이나 도구를 다시 호출하지 않는다. 수정 전 실패·수정 후
저장/재시작/취소 검사와 독립 검토, native·Linux의 관련 32검사·합성
15흐름/62판정·lint와 공개 소스 Linux 제품 빌드를 통과하고 공개했다. 운영 배포는 별도다.
[업무 대기·재개 자리 관리](reviews/2026-11-16-mission-capacity.md)를 공통 mission 정책으로
모았다. 새 업무 접수, 사용자 답변/연결/기기 메일 대기, 자동·명시적 재개가 같은
계정별 제한을 따른다. 전체 한도는 새 업무에만 적용하고 이미 맡은 업무는 그 안에서
대기·재개할 수 있다. 일시정지 업무의 자리와 접수 순서, 기존 지침·예산·영수증을
유지한다. 통합 전후 동일 시나리오, 독립 검토, native 계약 검사·합성 15흐름/62판정과
공개 소스 Linux 검사·lint·빌드를 통과하고 공개했다. 운영 배포는 별도다.
코드 통폐합도 완료 조건에 포함한다. [공통 Store 재시도](reviews/2026-11-16-store-retry-consolidation.md)로
실행기의 출력·사용량·종료·권한 검사와 메일 영수증 저장의 반복 루프 7곳을 하나로
합쳤다. 로컬 저장은 Busy만 재시도하고, 실행 권한 재검사는 기존 Unavailable 처리도
유지한다. 결과 불명 저장을 다시 쓰거나 메일/모델 요청을 재전송하지 않는다.
독립 검토·native 계약 검사·합성 15흐름/62판정과 공개 소스 Linux 검사·lint·빌드를
통과하고 공개했다. 모델 의미 품질이나 운영 배포를 뜻하지 않는다.
기존 공개 15세계의 대화·기억·업무·스킬·예약·모니터링을 각각 실행하고,
선택/제외·미실행·판정기 실패를 보존한다. 같은 반복 업무가 답변을 기다리는 동안
다음 회차가 중복 실행되던 결함을 고쳤으며, 다른 예약은 계속 진행하고 같은 예약은
최신 대기 회차 하나만 보존한다. [수정 전 실패와 검증 기록](reviews/2026-11-16-feature-stability.md)을
따른다. 실제 모델의 기억 대상 선택, 외부 provider·기기·장기 실행 수락은 별도로
남아 있다. native의 전체·기능별 평가와 반복 업무 회귀, 독립 검토를 통과했으며,
공개 소스 Linux의 계약 검사·lint·CLI 빌드·전체/기억 평가도 통과하고 공개했다.

[재개 연속성](reviews/2026-11-16-continuation-integrity.md)을 보강했다.
같은 업무가 재개된 뒤 추가 지시 초안을 보내지 못하는 화면 결함을 고쳤다. 아직 안 보낸
초안만 같은 업무의 새 실행을 따르며, 접수 여부가 불명확한 요청은 원래 식별자·내용을
유지한다. 기기 Gmail 대기 뒤 누락되던 저장 자료의 근거 주소를 보존해 중복 조회 없이
계산을 이어가고, provider 연결 대기 Store 경로도 기존 지침·예산을 보존하도록 통합했다.
이 연결 대기 수정은 기반 경로이며 새 일반 UI 기능을 뜻하지 않는다. 이전 미션 검사10실패는
과거 DB 구성7건과 조기 거절 기대값3건의 문제로 정리했고 제품 권한을 완화하지 않았다.
수정 전 실패·수정 후 결과, 독립 검토, 통합 native 검사·합성15흐름/62판정·웹45검사·
lint·웹/CLI 빌드가 통과했다. 인증 기능을 켠 빌드에서 일반 대화의 추가 지시가
막히던 문제도 원래 요청별 도구 선택으로 고쳤고, 실제 인증 경로와 권한 검사는 유지했다.
공개 소스와 새 웹 자산의 Linux 통합 검사·lint·빌드도
통과하고 공개했다.
운영 배포나 전체 실제 모델 품질 수락은 아니다.

[조율·도구 연결부](reviews/2026-11-16-coordinator-routing.md)를 보강했다.
기존 문서 읽기→수정→저장의 경로와 선택 문서/명시한 이름의 충돌을 바로잡았고,
반복 인용된 문서명이 선택 대상을 바꾸는 우회도 막았다. 맡긴 작업의 native Gmail
도구 누락과 완료·대기·재개 뒤 원래 채팅의 낡은 추가 지침 대상을 수정했다.
기억·문서·미션 분류기의 공통 실행 코드를 합치고, 각 단계의 권한을 재확인하면서
업무의 HTTP 연결을 재사용한다. 수정 전 실패와 수정 후 결과, 독립 검토,
통합 native 경로 검사·합성15흐름/독립 판정62검사·lint·CLI 빌드를 확인했다.
넓은 Store 미션 검사에서 나온 기존10실패는 수정 전 재현과 함께 별도로 기록했다.
공개 소스 Linux12단계 계약 검사·lint·빌드도 통과하고 공개했다.
실제 모델 품질·운영 속도 측정이나 배포는 아니다.

[연속 작업·하네스 보강](reviews/2026-11-16-adaptive-continuity.md)을 추가했다.
새 실모델12요청의 최초8통과/4실패를 보존했다. 문서 제목의 “계산”을 작업 지시로
오인한 저장 실패와 직전 문서 수정 경로를 고쳐, 원래 계산→저장→제목 변경→추가→조회
5요청이 실제 Terra/high에서 모두 통과했다. 단일 문서의 버전1→2→3과 본문·출처를
보존한다. 모호한 보완 답변·시간대 질문을 재시작 뒤 이어가고, 전체 질문 취소는 모델0회로
처리한다. 원문 철회 후 낡은 알림과 만료 동의가 새 모니터링 제안을 가리는 결함도 고쳤다.
요청 보완/취소·문서 대상·모니터링 유효성 로직을 통합했다. 새 수리 견적 선택 시나리오는
서버가 살아 있는 상태에서 늦은 응답의 작업자 종료를 관측한 뒤 취소·알림 차단을 검사한다.
최종 native 공개 평가15흐름·독립 판정62검사, 변경 경로 검사·lint·CLI 빌드를 통과했다.
독립 검토와 공개 소스의 Linux11단계 계약 검사·lint·CLI 빌드도 통과하고 공개했다.
운영 배포나 전체 비서 품질 수락을 뜻하지 않는다.

[생활 시나리오·실행 통합](reviews/2026-11-16-household-assistant.md)을 추가했다.
새 실모델13요청의 최초12통과/1실패를 보존하고, 실패한 “우산 챙기기” 알림 보완을
수정했다. 원래 보완·초안 취소4턴 재검증에서 정확한 알림 하나를 보존하며 모델0회로
처리했다. 새 실행·재개 실행의 완료 처리와 일정/문서/기억 분류 계약을 통합하고,
빈 최종 답변·잘못된 계획·추가 지침 재개가 멈추는 경계를 고쳤다. 환불 상태 후속
확인의 동의·무변화 점검·변경 알림·중지·재시작을 공개 평가에 추가했다. native 평가
14흐름·독립 판정53검사와 변경 경로 검증·lint·CLI 빌드가 통과했다. 원문 철회 중
도구 집계와 알림 업무 링크·모순된 상태 답변의 판정 누락도 수정했다.
독립 검토와 공개 소스의 Linux11단계 계약 검사·lint·CLI 빌드를 통과하고 공개했다.
운영 배포나 전체 비서 품질 수락은 아니다.

[일상 비서 시나리오·구조 통합](reviews/2026-11-16-everyday-assistant.md)을 완료했다.
공개 평가13흐름과 단계별 호출 한도·독립 판정 검증이 통과했다. 추가 지침·보완 답변·
재시작 뒤 같은 업무 이어하기, 직전 알림 수정/취소를 확인한다. 새 실모델11요청의
최초8통과/3실패를 보존하고, 실패 원인을 고친 뒤 원래 알림3단계와 미지원 요청의
4개 재검증을 통과했다. 알림은 모델0회다. 도구 노출/실행 정책과 startup/restore
이전 목록을 통합하고, 재개한 업무의 도구 오류 뒤 실행 상태가 남는 결함도 수정했다.
독립 경계 검토·native 검사와 공개 소스의 Linux 통합 검사·lint·CLI 빌드를 통과했다.
운영 배포나 전체 품질 수락은 아니다.

[작업 중 지침·후속 확인](reviews/2026-11-16-live-guidance-monitors.md)을 연결했다.
지원하는 읽기 대화·업무에서 추가 지침을 즉시 접수하고 같은 실행의 다음 처리 단계에
반영하며, 원래 자료 읽기·도구 결과·예산을 유지한다. 완료된 자료 조회 업무는 후속 확인을
제안하고, 소유자가 허용한 자료/연결 스레드·주기·만료 안에서 선택한 재사용 동의를 따른다.
변화 없는 점검은 모델0회이며 새로 동기화된 내용만 제한적으로 검토한다. 실제 Terra/high
추가 지침과 변경 감지 결과, 브라우저 접수/반영 및 중지 후 알림 차단을 검증했다.
공개 소스의 Linux 통합20검사·복원·lint·CLI 빌드도 통과했다.
원격 편지함/웹의 범용 자동 감시나 외부 작업 권한 확장은 포함하지 않는다.

[후속 연속 작업 평가](reviews/2026-11-16-live-continuity.md)에서 새 합성12요청 중
10개가 최초 조건을 충족했고, 실패한 직전 알림 수정·취소를 고쳤다. 동일3문장 재검증에서
같은 알림을 수정·취소하고 모두 모델0회로 처리했다. 이미지 하나의 업로드가 다음 이미지
준비를 막던 문제와 오래된 보관함 응답의 상태 덮어쓰기, 이미 완료 영수증이 있는 업무의
재시작 시 불필요한 모델 인증 의존도 함께 수정했다. 전체 품질 수락·운영 배포와는 구분한다.

[비서 주요 흐름 재검증](reviews/2026-11-16-assistant-main.md)에서 대화 재시도·삭제의
상태 충돌과 지연 응답 잠금, 만료 기억의 대상 누락, 날짜 문맥 부재, 잘린 업무 응답의
오완료·기존 미검증 checkpoint 재실행을 수정했다. 리눅스와 실제 브라우저 복구 검증을
통과했다. 같은 Terra/high 개발 요청에서 후속 대화2→1회, 기억 조회3→2회,
자료 계산5→3회로 모델 호출이 줄었다. 기억 조회는 검색 결과 근거가 정확하지만 기존
전체 본문 읽기 평가 조건은 충족하지 않아, 전체 품질 통과로 표시하지 않는다.

[응답 경로 후속 수정](reviews/2026-11-15-response-pipeline.md)으로 전체 도구 구성의
호출 전 실패, 멈춘 스트림/기록 조회, 정상 연결 뒤 재연결 중단과 전송 직후 기록 초기화를
고쳤다. 실제 브라우저의 연속 전송에서 기록 재조회·SSE 재연결 없이 기존 답변과
새 스트리밍 답변이 유지됐다. 모델 통신은 같은 클라이언트의 검증된 연결을 재사용한다.
[비서 핵심 복구 수정](reviews/2026-11-15-assistant-core-recovery.md)으로 개인 구독의
후속 라운드에도 현재 권한을 확인하며 연결을 재사용한다. 전송 접수 뒤 멈춘 HTTP가
입력창을 붙잡는 문제, 인증 갱신 취소 뒤 불필요한 대기, 진행 안내만 나온 응답의 잘못된
완료 처리와 마지막 도구 결과 뒤 최종 답변이 차단되는 문제를 수정했다.
새 대화 생성은 schema71의 사용자별 요청 영수증으로 재시도·새로고침 뒤 같은 대화로
복구하며, 삭제 영수증은 과거 snapshot 복원 뒤에도 중복 생성·부활을 막는다.
일반 질문의 “부탁해” 표현만으로 추가하던 업무 분류 모델 호출을 제거했다. 해당 경로의
호출 수와 기존 예약 해석을 Linux 실제 Runtime에서 확인했으며 실모델 속도 비율을 주장하지 않는다.
[호출 감소 검증](reviews/2026-11-13-conversational-missions.md#2026-11-14-ordinary-polite-request-routing).

[새 독립 합성 요청의 실모델 평가](reviews/2026-11-15-live-agent-wave4.md)를 통해
요일이 포함된 기억 정정의 잘못된 일정 분류, 출처 분리를 나눗셈으로 오인하던 보고서,
숫자 부분 문자열의 잘못된 모호성, 발송 금지가 로컬 초안까지 막던 접수를 고쳤다.
수정 후 같은13개 요청에서11개가 통과했고 수신인 보완 초안은 별도 합성 메일 fixture로
검증했다. 당시 남은 query/receipt 회복은 [후속 수정·비교](reviews/2026-11-15-source-repair-latency.md)에서
새 source 계산 도구와 설명용 추론 제거로 보완했다. 실패했던 원래 B 보고서 저장과 새
A/B 채택→C 실제 학습 사용을 검증했고, 동일 보고서 A/B는 모델5→2회와22.15→9.38초·
23.79→12.57초를 관측했다. 단순 대화는1회 그대로이며 운영 전체 속도 향상 주장은 아니다. 최초 실패·설정 실패·노출 사례 재평가를 구분하며
운영 전체 응답 속도 개선이나 새 holdout 통과로 주장하지 않는다.

[실모델 개발 평가](reviews/2026-11-15-live-agent-quality.md)에서 발견한 미래 읽기 요청의
고정 알림 오분류와 아침 기억 주제의 불필요한 일정 분류를 수정했다. 대상 기억 삭제·
삭제 후 회상·실제 예약 브리핑의3개 후속 실모델 조건과 Linux 관련 검사/Clippy/빌드를
통과했다. 최초 평가 실패와 후속 재검증은 분리해 보존하며 전체 출시 품질 수락은 아니다.

[2차 실모델 평가](reviews/2026-11-15-live-agent-wave2.md)는 새 합성8세계의22개 요청을
고정하고 최초 엄격 판정8통과·10실패·4미실행을 기록했다. 분류기 실행 단계 충돌,
기억 대상의 소유 표현 누락, 정확한 소수의 표시 형식 때문에 학습되지 않던 문제를
고친 뒤 새 DB에서 전체를 재실행해22/22·8/8세계가 독립 판정을 통과했다. 별도로
실제 예약의 학습 스킬 호출·철회 정지·명시적 재개 후 새 실행도 확인했다.
동일 사례의 개발 회귀 결과이며 새로운 holdout·운영 구독 연결 수락은 아니다.

[3차 실모델 개발 평가](reviews/2026-11-15-live-agent-wave3.md)에서 자연스러운 한글 수량 표현이
근거 계산·보고서 저장·학습으로 이어지지 않는 지원 공백을 확인했다. `4.08을`, `4개를`,
`2.64야` 같은 완전한 숫자를 지원하고, `4만` 같은 모호한 배수 표현이나 숫자 일부는
거절하도록 보완했다. 최초 엄격 판정3/9와 수정 후 같은9개 요청의9/9를 구분해 기록한다.
보고서 저장과 실제 A/B 학습→새C 재사용을 확인했으며 새 holdout나 출시 품질 점수는 아니다.

[빠진 값 이어하기](mission-input-continuation.md)는 이미 요청한 일회성 초안/개인 일정에서
수신인·날짜·시각·길이를 원래 업무 카드로 물어보고, 답변 뒤 같은 실행의 남은 단계를 이어간다.
대기 중 모델 호출 없이 다른 일을 진행하고, 새로고침·재시작·응답 유실에도 원래 결과와 한도를
유지한다. 원래 값 덮어쓰기와 계정 주소를 수신인으로 오해하던 경계를 독립 검토 후 고쳤다.
[검증 범위](reviews/2026-11-15-mission-input-continuation.md). 이제 일반 맥락·저장 원문 선택·관측한 일정 대상 선택도 원래 카드에서 답할 수 있다. 반복 읽기의 현재 회차만 기다리고 다음 회차에 답을 복사하지 않는다. 소식함/opt-in Push가 원래 카드로 연결되며 답변·취소·철회 뒤 다시 질문하지 않는다. [확장 검증](reviews/2026-11-15-flexible-mission-clarification.md).

[추가 구현 계획](additional-capabilities-plan.md)은 복합 업무의 산출물 저장, 계정 선택/대기,
별도 native 모델 읽기, 기존 메일/Calendar 실행, 복합 스킬 학습/공유, 인증 브라우저를
A–F의 실제 사용자 결과로 나눴다. A의 조사·계산→요청한 비공개 보고서 저장을 구현했다.
실제 읽기 영수증과 문서 revision을 원래 맡긴 일에 연결하고, 내용은 검토 필요로 표시한다.
중복 저장·원문/대화 철회·schema58 이전과 복원을 Linux에서 검증했다.
B의 연결 Gmail 계정 선택·같은 업무 이어하기도 구현했다. 원래 실행/예산을 유지하고
여러 계정·연결 필요·대기 만료를 원래 카드에서 처리한다.
C의 별도 native Gmail 검색/본문/thread/PDF·텍스트 읽기와 같은 대화·위임 업무 재개,
D의 요청한 로컬/Gmail 초안·기본 개인 일정 작업, E의 근거 계산→보고서/초안 구조 학습을
통합했다. F는 선택한 서버 페이지를 원래 업무로 가져오기와 기기 내 별도 브라우저의
세션/기록/제한 관측과 **Gitea1.24.6의 볼트 password/TOTP 로그인·정확한 저장소 issue 개수 읽기·세션 재사용**을 구현했다. 실제 격리된 합성 계정으로 원래 채팅 완료까지 검증했다. [범위와 사용법](native-credential-gitea.md)·[검증](reviews/2026-11-15-native-gitea-login.md). 다른 사이트·일반 폼/브라우저 미션은 후속이다.
[통합 검증과 남은 경계](reviews/2026-11-15-compound-capabilities.md).
새 합성 보고서의 실모델 검사에서 완료된 검색을 반복하던 경로를 고쳐, 같은 사례를
새 DB에서 원문 읽기→정확 계산→문서 저장까지 재검증했다. 모델/effort는
`gpt-6.1-terra/high`, 사람 검토 상태는 유지한다. 최초 실패와 수정 후 재실행을
[기록](reviews/2026-11-15-compound-capabilities.md#actual-model-development-check)하며
새 holdout나 전체 품질 점수로 주장하지 않는다.
[보고서](compound-reports.md)·[검증](reviews/2026-11-15-compound-reports.md),
[연결 대기](connection-waits.md)·[검증](reviews/2026-11-15-connection-waits.md).

## 현재 지원하는 개발 프리뷰

| 영역 | 구현한 범위 | 남은 경계와 사용·증거 |
| --- | --- | --- |
| 설치·계정 | Rust CLI/API·내장 React·SQLite single writer, 최초 admin, 로컬 초대·임시 암호/강제 변경, 세션 유지·철회, 계정 정지/재개·역할·암호 재발급 | 전체 계정 삭제·개인 자료 정리·볼트 삭제 outbox/ACK를 구현했다. 최종 승인 후 gws 초대 메일·로컬 서명 bundle 설치/schema47부터 현재까지 stopped update/데이터 보존 제거도 구현했다. 서명된 한 줄 설치 preview를 공개했다. 안정판·실계정 수락은 남음. [개발](development.md)·[계정](account-management.md)·[검증](reviews/2026-11-13-account-management.md) |
| 모델·대화 | 공유 OpenAI/Anthropic API·OpenRouter·compatible/local 설정, 실제 text/tools/image 연결 확인, model/effort pin, 사용량·실제 실행 단계 시간/누락 집계, 저장 채팅·SSE·취소·재시도·초안 복구·격리, 오래된/다른 소유 대화의 제한 검색·읽기, 개인 ChatGPT 로그인·sealed session·text/tool proof·선택·native Linux 전달·foreground 도구·기억/자료·제한된 Calendar 변경·명시적 일회/반복 읽기 미션 | 개인 Claude 공식 CLI의 설정·text probe·선택·저장 대화도 구현했다. Claude 실제 rootless Linux/공식 로그인과 media/background·실계정 및 종합 실모델 품질 검증은 미완료. 모델 이름으로 capability를 추정하지 않는다. [모델](provider-settings.md)·[전송](model-transport.md)·[plan protocol 검증](reviews/2026-11-14-chatgpt-plan-protocol.md)·[대화](chat.md) |
| 기억·자료 | 사용자별 원문/근거·기록 시각별 검색·선택적 고정 한국어 별칭, 직접 기억·정정·망각·대화의 제한된 절대 날짜 적용, opt-in literal 자료 추출·예산·망각 억제, 문서/목표/요청 편집·내보내기·지원 외부 읽기의 보고서 저장 | 넓은 대상 해석·추출 정확도·장기 문맥/시간 사실의 독립 평가와 자동화 전체 완료는 아니다. [지식](knowledge.md)·[기억](conversational-memory.md)·[추출](automatic-memory.md)·[문서](artifacts.md) |
| 도구·스킬 | 검증된 bounded tool loop, 내장 절차·정확한 소수 계산, 개인 curated/public GitHub pin·update/rollback/disable/remove, 로컬 작성/check·정적 catalog 생성·설정한 공개 목록의 발견/원본 확인 | 정확 계산·저장 자료 조회·일본우편 최근 요약의 개인 학습/두 입력 proof·새 입력 재사용은 구현. 알려진 절차의 합성 공유 파일 내보내기도 구현. 실제 사용한 스킬의 예약 정지/복구와 참조 없는 공개 snapshot 정리를 구현했다. 2–4단계 정확 계산과 유일한 저장 자료의 숫자를 읽는 근거 계산의 구조 학습·새 입력 재사용·합성 export도 구현했다. 공개 URL2~4개의 실제 수집·두 사례 학습·새 대상/명시한 미션 재사용·합성 export도 구현. 근거 계산→비공개 보고서/로컬 초안의 두 실제 채택 결과 학습·새 입력 재실행·합성 공유를 추가했다. 일반 외부 workflow 학습·전체 이력 보존/정리 정책은 남음. 공개 목록은 운영자가 설정하며 자동 설치하지 않는다. [업무](task-skills.md)·[community](community-skills.md)·[검증](reviews/2026-11-14-community-catalog-runtime.md) |
| 웹·브라우저·메일 | 공개 HTTP/Brave·검증된 Linux Chromium 읽기, 대화별 workspace·화면 보기·사람의 로그인/조작·같은 페이지 후속 대화, 개인 Gmail 읽기/thread·PDF/텍스트 첨부 페이지 읽기·로컬/Google 초안 CRUD·승인된 즉시/고정 예약 발송·불명 결과의 제한된 보낸편지함 조회 | 메일은 매번 최종 수신인/본문 등을 보고 명시적으로 허락해야 한다. 원래 수명 안의 명시적 profile 복구와 Linux 실제 재시작/강제 종료 후 복구를 확인했다. 선택한 페이지를 원래 업무의 새 근거로 연결한다. 기기 내 별도 브라우저는 고정 수명·빈 창 복구와 Gitea1.24.6의 로컬 볼트/TOTP 로그인·정확한 저장소 issue 수 읽기를 지원한다. 다른 사이트·자동 폼/장기 로그인 보장·실제 Gmail 계정 검증은 미완료. [웹](web-research.md)·[브라우저](browser-workspaces.md)·[메일](mail.md) |
| 공식·custom 자료 연결 | 개인 Gmail/Slack 수집·custom ingest, 선택한 Codex 사용자 요청·정규화 통화 전사의 일회 수집, scope/cursor/receipt/provenance·상태 화면·철회, 일본우편 최근 상태와 확인한 공식 표의 표시 이력 조회 | 합성 Linux 검증은 실제 OAuth/계정 scope/rate 검증을 대신하지 않는다. 카톡 코드는 공개 패키지에서 제외. schema48에서 시작 ACK 유실 복구를 추가했다. 통화 원본 기기/포맷·실제 전사 품질·다른 작업 세션 형식·다른 배송사·현재 실제 배송물의 이력 조회 수락은 남음. [연결](data-connections.md)·[검증](reviews/2026-11-13-source-connections.md) |
| 알림·소식·Push | 한글/영문 제한 고정 알림 capture/control·일상 날짜/요일/한글 시각·명확한 주 범위 보존, 일회/간격/매일/매주·IANA/DST·latest-only 회차, inbox·quiet hours·예약별 소식 끄기/다시 받기, 저장한 followup 마감/재확인/미룬 날짜 알림, opt-in 기기 Push | 날짜 없는 요청·약속의 저장 근거 제안·개별 숨김·고정 주제 전체 끄기를 구현했다. 일반 의미 기반 제안 발굴·실제 휴대폰 설치/수신/백그라운드는 미완료. [예약](schedules.md)·[Push](web-push.md)·[소식 제어](reviews/2026-11-14-notification-topics.md) |
| 맡긴 일 | 양식/API와 현재 대화의 명시적 위임으로 일회성·반복 읽기/계산/공개 HTTP·명시한 개인 Gmail/기본 Calendar 읽기 업무 등록·제어, 실제 영수증과 좁은 정확 일치 알림 생략, 의미 조건은 검토 필요 | 빠진 값을 다음 답변으로 보완하고 직전 업무 영수증으로 제어한다. 일상 요일/주·한글 시각을 원래 시간대로 검증한다. 정지한 일회성 업무는 기존 한도 안에서 조건을 고쳐 다시 계획할 수 있다. 요청한 로컬/provider 초안·기본 solo-event 생성/날짜 이동과 원래 카드의 실제 작업 상태를 연결했다. 준비된 효과의 조건 변경은 충돌로 알리고 원래 결과를 보존한다. 넓은 자연어/범용 쓰기·browser/vault 자동 실행·Claude background는 미완료. 개인 ChatGPT의 명시적 읽기 미션은 지원한다. [맡긴 일](missions.md)·[일회성 검증](reviews/2026-11-13-owner-missions.md)·[반복 검증](reviews/2026-11-13-recurring-missions.md)·[대화 검증](reviews/2026-11-13-conversational-missions.md) |
| Calendar | 별도 개인 동의, 대화/직접 양식의 기본 캘린더 읽기·명확한 단일 timed/종일 solo-event 생성/수정/삭제, ETag/dispatch receipt·불명 결과 lookup | 확인 질문의 다음 답변·직전 일정 지칭·기간을 유지한 날짜 이동을 지원한다. 반복/참석자/보조 캘린더 쓰기·실계정 검증은 미완료. [Calendar](calendar.md)·[대화 검증](reviews/2026-11-13-conversational-calendar.md) |
| 미디어·PWA | 독립 image/UTF-8/PDF 텍스트 upload·preview/library·삭제 철회, 실제 vision probe, 녹음·미리 듣기·별도 OpenAI STT·수정 후 초안 삽입, PWA 글/링크/사진/파일/음성 공유받기·공개 offline shell·설치/업데이트 | 실제 음성 인식 품질·PDF OCR/시각 이해·휴대폰 OS 공유 메뉴·실기기는 미완료. 첨부 문맥은 도구/지속 쓰기를 제한한다. [이미지](media.md)·[파일](private-files.md)·[PWA](pwa.md)·[파일 검증](reviews/2026-11-13-private-files.md)·[PDF 검증](reviews/2026-11-14-private-pdf-files.md) |
| 독립 볼트 | 별도 Rust 암호문 서비스/DB/origin/session·WASM Worker·React, 항목/TOTP/로컬 CSV·키 교체, 독립 암호화 파일 export/import·봉인된 현재 service checkpoint·운영 중 암호화 snapshot/시점 복구·실험적 broker 기기/서명/철회 기반 | 실험적 native companion이 현재 독립 vault 또는 명시한 암호화 archive의 선택 credential로 정확한 GitHub 저장소를 읽고 명시적 채팅 요청의 같은 실행에 결과를 돌려준다. 주기적 pinned SSH journal receiver를 구현했다. 기기 내 Google INBOX 개수와 별도로 선택한 최대5개 메일 미리보기도 구현했다. 선택한 일반 텍스트 본문 읽기와 별도 연결의 최종 승인된 즉시/고정 예약 Gmail 발송·불명 결과 조회를 구현했다. 별도 명시한 모델 읽기 grant의 Google 검색/선택 본문/thread/PDF·텍스트를 원래 실행에 연결했다. 일반 credential·임의 쓰기·실제 off-host 배치·공식 서명 client 배포·실기기는 남음. 구 v1 암호화 파일의 기기 내 이전을 구현했다. served JS의 악성 서버 한계는 유지. [볼트](vault-service-client.md)·[archive](vault-archive.md)·[archive 검증](reviews/2026-11-13-vault-archive.md)·[service 복구 검증](reviews/2026-11-13-vault-service-recovery.md) |
| 복구 | fsynced auth/삭제 journal, stopped age export·recipient key epoch, 봉인된 현재 checkpoint와 수동 최신 journal을 고정한 과거 export 복원·격리, SQLite 이미지/파일/문서 보존 | 과거 사용량 불명을 유지하면서 운영자가 새 호출·음성 길이 한도를 부여할 수 있다. 주기적 live journal·고정 SSH receiver와 실행 중 전체 자료의 암호화된 로컬 export를 구현했다. 실제 off-host 배치·무손실 최신 삭제·live 전체 DB export의 실제 운영 수락·실운영 이전은 남음. 원본을 영구 봉인하는 cutover와 일반 재해복구를 구분한다. [backup](backup.md)·[restore](restore.md)·[검증](reviews/2026-11-13-sealed-restore.md) |

## 이번에 반영한 기능

| 단위 | 반영 내용과 범위 | 증거 |
| --- | --- | --- |
| 브라우저 대화 스트림 복구 | 반쯤 끊긴 SSE의 15초 연결/45초 무수신·복구 GET 전체 30초 제한, 실제 이벤트·지연 heartbeat 뒤 연속 실패 수 초기화, 새 전송의 SSE 접수 확인 시 현재 스트림·이전 페이지 보존 | [합성 검증·한계](reviews/2026-11-15-responsive-chat-stream.md) |
| 빠진 값 답하고 업무 계속하기 | 원래 카드의 수신인/날짜/시각/길이 질문·새로고침 복구·같은 실행 재개·원래 값/예산 보존 | [사용법](mission-input-continuation.md)·[검증](reviews/2026-11-15-mission-input-continuation.md) |
| 한글 수량과 근거 계산 | 조사·단위·서술 어미가 붙은 완전한 수치 처리, 부분 숫자·배수 모호성 거절·새 원문 학습 재사용 | [검증](reviews/2026-11-15-live-agent-wave3.md) |
| 연결된 복합 비서 업무 | native 메일 대기/재개·초안/일정 실제 결과·두 사례의 구조 학습·선택 페이지 연결·기기 브라우저 세션 | [통합 검증](reviews/2026-11-15-compound-capabilities.md)·[초안/일정](compound-actions.md)·[학습](compound-learning.md) |
| 계정 선택·원래 업무 이어하기 | 명시한 Gmail 계정 우선, 유효한 단일 연결 재사용, 다중 계정 선택·연결 대기, 원래 호출 수/만료·공정 재개·선택 결과 재확인 | [사용법](connection-waits.md)·[검증](reviews/2026-11-15-connection-waits.md) |
| 복합 조사 보고서 | 명시한 요청의 자료 읽기·정확 계산·비공개 문서 하나 저장, 실제 단계/출처/문서 revision·검토 상태, 중복·철회·복원 경계 | [사용법](compound-reports.md)·[검증](reviews/2026-11-15-compound-reports.md) |
| 실모델 기억·업무·학습 후속 개선 | 문서/업무 분류 단계 연결, 소유자 기억 대상 보존, 정확 소수 표시와 학습 근거 일치, Calendar 후속 답변 경계·schema58 이전/복원 | [최초 실패·수정 후 전체 재실행](reviews/2026-11-15-live-agent-wave2.md) |
| 연결 도구의 대화 호환성 | Gmail 첨부 도구의 지원하지 않는 모델 요청 형식 수정, 기존 읽기 제한을 유지하며 공유/개인 연결의 대화 계속하기 검증 | [검증](reviews/2026-11-14-combined-tool-profiles.md)·[전체 구성 회귀](reviews/2026-11-15-complete-tool-profiles.md) |
| 일본우편 표시 배송 이력 | 공식 상세 주소를 한 번 읽어 확인한 표의 사건별 날짜·상태·장소를 보존, 모르는 형식·번호 불일치는 실패 표시 | [사용법](parcel-tracking.md)·[검증](reviews/2026-11-15-parcel-history.md) |
| 기록에 근거한 검토 제안 | 날짜 없는 요청·약속을 저장하면 현재 기록으로 연결되는 제안, 개별 숨김·주제 전체 끄기, 철회/조용한 시간/Push 중복 정책 | [사용법](proactive-suggestions.md)·[검증](reviews/2026-11-15-proactive-suggestions.md) |
| 기기 볼트로 승인한 메일 발송 | 별도 Google 발송 연결·기존 최종 내용/시각 승인·정확한 한 번의 기기 전송·불명 결과 조회·복원 시 재발송 방지 | [사용법](native-approved-gmail-send.md)·[검증](reviews/2026-11-15-native-approved-gmail-send.md) |
| 일상적인 날짜·시간 입력 | 오늘/내일/모레·이번/다음 주 요일·한글 시각을 알림/맡긴 일/Calendar에서 공통 해석; 빠진 값 후속 답변·원래 시간대 보존 | [사용법](conversational-civil-time.md)·[검증](reviews/2026-11-15-conversational-civil-time.md) |
| 공개 페이지 수집 학습 | 현재 URL2~4개의 실제 관측·개별 실패, 서로 다른 두 사례 뒤 새 대상으로 재사용·명시한 읽기 미션·합성 공유 | [사용법](public-page-recipes.md)·[검증](reviews/2026-11-15-public-page-recipes.md) |
| 기기 Gmail 본문 | 미리보기에서 직접 고른 메일의 일반 텍스트를 원래 대화/설정에서 열기, 별도 권한 자동 재사용·기기 내 credential 보존 | [사용법](native-google-message-body.md)·[검증](reviews/2026-11-15-native-google-body.md) |
| 독립 native 패키지 | 볼트 서버·credential companion의 별도 서명/target 검증·HTTPS 다운로드와 출처 연결; 설치/실행/연결은 명시적 다음 단계 | [사용법](component-packages.md)·[검증](reviews/2026-11-15-component-packages.md) |
| 외부 자료 보고서 저장 | 공개 HTTP·연결 Gmail/기본 Calendar 읽기에서 실제 문서 저장·수정, 정확한 근거 보존·연결/대화 철회 연동 | [사용법](external-reports.md)·[검증](reviews/2026-11-15-external-reports.md) |
| 학습 절차로 맡긴 일 수행 | 검증된 개인 로컬 절차의 일회성·반복 실행, 현재 조건/원문 재조회·실제 결과 검증, 사용한 스킬 철회 시 관련 예약만 정지 | [사용법](missions.md#학습한-로컬-절차로-맡긴-일-수행)·[검증](reviews/2026-11-15-mission-recipes.md) |
| 서명 bundle 다운로드 | 명시한 HTTPS 버전·독립 공개키 pin, 코드 실행 없이 검증 후 새 경로 저장 | [사용법](signed-download.md)·[검증](reviews/2026-11-15-signed-download.md) |
| 브라우저 중단 복구 | 운영자 opt-in 전용 profile·원래 앱 session/만료 한도·빈 화면 직접 확인, 과거 입력/이동 재생 없음; Linux 실제 Chromium·앱 재시작/강제 종료 복구 확인 | [사용법](browser-workspaces.md)·[검증](reviews/2026-11-15-browser-recovery.md) |
| Gmail 첨부 읽기 | 메일 본문과 파일을 분리해 PDF/UTF-8 텍스트를 필요한 부분만 조회; 채팅 도구·메일 화면·명시한 읽기 미션 연결, 발송 승인 유지 | [메일](mail.md)·[검증](reviews/2026-11-15-gmail-attachments.md) |
| 한국어 별칭 검색 | 휴대전화/휴대폰/핸드폰 등 공개 세 단어군을 선택적으로 함께 검색; 문구의 나머지 순서·범위·페이지를 유지, 화면과 에이전트 도구 연결 | [사용법](korean-search-aliases.md)·[검증](reviews/2026-11-15-korean-search-aliases.md) |
| 자료 기록 시각 검색 | 원문의 관측 시각으로 시작 포함·종료 제외 기간 검색; 기억의 유효 시각과 구분, API·도구·화면에 같은 범위 적용 | [사용법](source-time-filter.md)·[검증](reviews/2026-11-15-source-time-filter.md) |
| 날짜를 정한 기억 | 대화에서 명시한 절대 날짜를 원래 시간대로 저장·정정, 적용 전/만료 후 현재 기억에서 제외, 재시도·재시작에도 날짜 유지 | [사용법](conversational-memory.md)·[검증](reviews/2026-11-15-dated-conversational-memory.md) |
| Linux ARM64 개발 빌드 | 비서·독립 볼트·companion 교차 빌드, QEMU에서 실제 초기화·로그인·세션/SQLite 재시작·삭제·별도 볼트 shell 확인; 물리 ARM·공식 release는 별도 | [빌드](linux-arm64.md)·[검증](reviews/2026-11-15-linux-arm64.md) |
| 맡긴 일 조건 수정 | 정지한 일회성 업무의 조건·확인 기준 변경, 원래 요청/이력 보존, 기존 호출·시간 한도 안에서 재계획·검증·보완 | [사용법](mission-revisions.md)·[검증](reviews/2026-11-15-mission-revisions.md) |
| 배포 고지 보충 | 정확한 lock 항목에 공개 원본 고지를 별도 출처로 결합; Rust17개·웹4위치의 보충 근거 확보, 기존21/66 누락과 strict 판정 유지 | [사용법](notice-supplements.md)·[검증](reviews/2026-11-15-notice-supplements.md) |
| 자료 읽기·정확 계산 학습 | 현재 요청의 문구로 유일한 원문·숫자 항목을 확인해 계산; 독립 두 사례 뒤 새 원문으로 재사용·합성 공유, 기존 학습은 generation3의 새 근거 필요 | [사용법](evidence-calculation.md)·[검증](reviews/2026-11-15-evidence-calculation.md) |
| 기기 Gmail 미리보기 | 별도로 연결한 계정의 제목·보낸 사람·날짜·짧은 내용 최대5개를 원래 채팅/설정에 표시; 이후 모델 문맥·회상·학습에서 제외 | [사용법](native-google-inbox-preview.md)·[검증](reviews/2026-11-15-native-google-inbox-preview.md) |
| 알림 후속 대화 | 빠진 시간·내용·제한된 오전/오후·날짜를 다음 답변으로 보완; 제공한 값·시간대·같은 요청 재시도를 유지 | [사용법](reminder-followups.md)·[검증](reviews/2026-11-15-reminder-followups.md) |
| 격리 비교 실행기 | 두 공통 세계의 소스 사본·실행 adapter·독립 판정 구현; 실제 비교는 Mac의 기존 공유 메모리 슬롯 부족으로2건 미실행 | [사용법](agent-evaluation.md#두-공통-세계의-로컬-실행-adapter)·[기록](reviews/2026-11-15-paired-baseline.md) |
| 웹 의존성 고지 | 두 React 앱의 lock/설치본을 오프라인 대조,217개 고지와66개 미완료 위치를 보고; 법적 검토·완전한 배포 고지와 구별 | [사용법](web-license-evidence.md)·[검증](reviews/2026-11-15-web-license-evidence.md) |
| 채팅 우선 로딩 | 최초 JavaScript610,903→383,331byte(37.25% 감소), 보조 화면 오류에도 초안 보존; 모델 지연/전체 PWA 크기 측정과 구별 | [검증](reviews/2026-11-15-deferred-workspace.md) |
| 배송조회 학습 | 독립 두 조회의 결과로 개인 절차 학습, 새 현재 번호의 실제 재조회와 구조 공유; 일본우편 최근 요약 범위 | [계약](learned-parcel.md)·[검증](reviews/2026-11-15-learned-parcel.md) |
| Google credential | 기기 내 볼트 credential로 계정/scope 확인 후 INBOX 개수만 원래 채팅에 전달 | [계약](native-credential-google.md)·[검증](reviews/2026-11-15-native-google-credential.md) |

다른 완료 단위와 상세 수정 경과는 [11월15일 수락 이력](reviews/2026-11-15-status-history.md)에
보존한다. 날짜별 검증 문서의 경로는 유지하며, 전체 상태를 확인할 때 이력을 먼저 읽을 필요는 없다.

## 계획 단계와 현재 위치

[기존 P0~P10 계획](plan.md#단계별-진행)과 [이전 순서](migration.md#단계와-종료-조건)를
작업 기준으로 사용한다. 아래의 부분 구현을 단계 전체 완료로 계산하지 않는다.

| 계획 단계 | 현재 구현 | 아직 닫지 못한 종료 조건 |
| --- | --- | --- |
| P0 공개 기반 | 공개 저장소·Rust/React/Linux 빌드·기여/OpenSpec·합성 결과 runner/독립 grader·실행 단계 시간/실패·누락 집계·기존149개 소스 내용 검사·두 세계 격리 실행 adapter | 실제 비교 실행(호스트 공유 메모리 부족)·넓은 사례/실모델·통계 평가 |
| P1 영속 상태 | SQLite 격리·실행/외부 영수증·journal·봉인된 현재 복원·수동 pin을 사용한 과거 복원·명시적 새 사용 한도 | 실제 off-host 배치·무손실 최신 삭제·live 전체 DB export의 실제 운영 수락 |
| P2 대화 | 계정·설정·스트림·취소/재시도·개인 ChatGPT·선택 Claude 공식 CLI text 실행 | 지원 구독의 전체 실행 범위·실계정 검증 |
| P3 자료·기억 | 출처·기억 정정/망각·제한 추출·문서·원래 시간대를 보존한 절대 날짜 기억·이전/다른 소유 대화의 근거 검색 | 넓은 시간 표현/장기 사실·추출 품질의 독립 검증 |
| P4 도구·학습 | typed 도구·bounded 실행·Git 스킬·제안·연결 Gmail/Calendar 읽기 미션·예약 스킬 의존성/정리·제한된 개인 학습/두 입력 proof/대화 및 예약 재사용·공개 페이지 묶음의 새 대상 재조회·근거 계산→보고서/초안 구조 학습 | 일반 외부 복합 절차·범용 학습 export·실모델 효용/품질 검증 |
| P5 예약·소식 | due loop·반복 회차·알림·빠진 값의 다음 답변 보완·quiet hours·예약별 소식 제어·followup 날짜 알림·Push | 전체 알림/제안 정책·실기기 수신·고정 메일 외 예약 외부 작업 |
| P6 외부 연결 | Gmail 본문/제한 첨부·Calendar·공식/커스텀 수집·선택한 Codex 요청·정규화 통화 전사 수집·공개 웹·일본우편 최근 상태·원래 수명 안의 브라우저 복구·승인된 gws 초대 | 통화 vendor 포맷/실제 전사·다른 작업 feed·다른 배송사/전체 이력·실계정 |
| P7 첨부·모바일 | 이미지/파일/PDF 텍스트·녹음·글/링크/파일/음성 공유 | 모바일/접근성·실기기 |
| P8 독립 볼트 | 암호문 서비스·클라이언트·CSV/TOTP·현재 복구·운영 중 snapshot/시점·명시적 최신 journal 복구·주기적 SSH journal receiver·구 v1 파일 이전·실험적 native GitHub/Google INBOX 개수·미리보기/선택 본문·별도 모델 읽기 grant의 검색/thread/첨부·원래 실행 대기/재개·승인된 Gmail 발송·기기 브라우저 세션 | 일반 credential·Gitea 외 사이트 로그인/읽기·임의 쓰기 도구·실제 옛 파일 수락·실제 off-host/무손실 최신 복구 |
| P9 이전·전환 | 정규화한 private bundle의 새 계정·원문/기억/문서 이력 이전·대조/격리 보고·합성 복구 | 실제 operator export·운영 대조·drain/cutover/rollback 실증 |
| P10 출시 | 개발 프리뷰·공개 문서·Mac/Linux 네 플랫폼 서명 설치 preview·양 언어 한 줄 설치·key-pinned HTTPS 다운로드·로컬 서명 설치/schema47부터 현재까지 update/제거·CycloneDX source-lock inventory·서명된 출처·ARM64 QEMU 실행 | 별도 vault/companion 배포·물리 ARM/Intel/WSL2·실제 서비스 수락·독립 빌드 증명/전체 license·품질/기기/안정판 gate |

수락한 Calendar 단위는 P6으로, broker 기반 단위는 P8으로 추적한다.
P0 합성 결과 실행기·독립 판정기와 두 세계 비교 adapter를 구현했다. 기존 비서의 실제 비교 실행은
호스트 공유 메모리 부족으로 미완료이며 넓은 품질 평가도 남는다.
P4의 정확 계산·저장 자료 조회·일본우편 최근 요약과2–4단계 계산 그래프, 유일한 원문 숫자에 묶은1–4단계 계산과 명시한 공개 URL2~4개 수집의 학습·새 입력 재사용·합성 공유를 수락했다. 작은 실모델 개발 평가에서 계산/원문 학습과 예약 재사용도 확인했다. 일반 외부 복합 업무·넓은 실모델
품질 검증은 남은 계약으로 유지한다. 새로운 내부 작업명을 제품 계획의 P 번호로 사용하지 않는다.
CI는 사용자 지시에 따라 계속 비활성이다.

## 다음 구현과 출시까지 남은 일

진행 중인 단위는 위 표의 단계에 연결하고, 다음 미완료 계약을 기존 계획의 의존순서대로
닫는다. `선택` 연결/볼트 패키지도 v1 제공 대상이며 사용자의 설치 선택을 뜻한다.

1. **P0 평가 기준·P1/P2 기초의 남은 계약**: 기존 소스의 내용 일치 검사는 구현했다.
   이를 고정한 로컬 사본·두 공통 fixture adapter와 실행 설정을 연결했다. 실제 비교는
   Mac의 공유 메모리 슬롯32개가 이미 사용 중이라2건 모두 미실행이다. 다른 자원이나
   시스템 설정을 변경하지 않았으며, 환경이 준비된 뒤 같은 고정 비교를 실행해야 한다.
   초기 독립 상태/영수증 grader와 실패를 포함한 관측
   latency/usage 집계와 실제 서버 단계별 계측은 구현했다. 2차까지의 작은 실모델 개발 평가와
   실패 수정/전체 재실행은 기록했으며,32세계×3회 pilot·새 독립 holdout과
   브라우저 표시 시간·성능 개선은 별도로 이어간다.
   수동 과거 복원과 명시적 새 사용 한도를 수락했다.
   주기적 live journal/독립 SSH receiver와 실행 중 전체 자료의 암호화된 로컬 export를 구현했다. 실제 off-host 배치·무손실 최신 삭제와
   지원 구독 실행 범위는 해당 저장/연결 의존성과 함께 닫는다.
2. **P3/P4 기억·학습·업무 수행**: 초기 합성 평가와 계산/자료 조회의 typed 학습·
   두 입력 proof와 대화 기억/전이 망각/업무 취소/학습 재사용의 실제 결과 평가를 구현했다.
   명시적 날짜의 대화 기억·정정과 현재/과거 조회 및 원래 시간대 복구를 구현했다.
   자료의 기록 시각 범위와 선택적 고정 한국어 별칭을 API·읽기 도구·검색 화면에 연결했다.
   지원 외부 읽기에서 문서 하나를 저장·수정하고 실제 출처 철회를 전파하는 흐름을 구현했다. 넓은 시간 표현·실모델 이해력·일반 복합 외부 절차의 독립 평가로 확장한다.
   정확 계산 그래프·저장 원문에 묶은 계산·명시한 공개 URL2~4개 수집의 두 사례 학습/새 입력 실행/합성 export는 구현했다. 일반 외부 복합 절차·전체 이력 보존/정리 정책은 남는다.
   정지한 일회성 업무의 기준 수정·기존 한도 내 재계획/검증/보완과 검증된 로컬 학습 절차의 일회성·반복 실행을 구현했다.
   제한된 여러 턴 명확화와 일상적인 날짜/요일/이번·다음 주/한글 시각을 공통으로 검증한다. 더 넓은 시간 해석·일반 복합 작업으로 이어간다.
3. **P5/P6 예약·외부 연결**: Gmail의 제한된 PDF/텍스트 첨부 읽기를 구현했고, 자료 수집은 본문만 색인한다. 날짜 없는 요청·약속의 고정 주제 제안과 소식 제어를 구현했다. 더 넓은 의미 기반 제안 발굴,
   구현한 Gmail provider draft·승인된 고정 예약 발송과 Calendar 후속 명확화의 실계정 검증,
   선택한 Codex 요청과 정규화 통화 전사 수집·schema48 시작 ACK 복구는 구현했다. 통화 vendor 포맷/실제 전사·다른 작업 형식(Claude 원본 로그의 사람/자동화 구분은 [공식 형식의 한계](local-work-sessions.md#claude-transcript-provenance-limit))·
   일본우편의 확인한 공식 표 표시 이력을 구현했다. 현재 실제 배송물의 성공 조회·다른 배송사와 실제 OAuth/계정 검증은 남는다.
   브라우저는 운영자가 선택한 배포에서 원래 세션·수명 안의 로그인 정보 복구를 구현했으며,
   장기 로그인 보장이나 인증된 자동 폼 실행은 포함하지 않는다.
   공유 provider와 개인 ChatGPT의 명시한 Gmail/기본 Calendar 읽기 미션은 구현했다. 일회성 미션의 요청한 로컬/provider 초안·기본 개인 일정 생성/날짜 이동도 기존 작업 원장에 연결했다.
   미션의 자동 메일 발송·반복/참석자/보조 캘린더 쓰기와 Claude background는 현재 지원 범위에 포함하지 않는다.
4. **P7 첨부·모바일**: 파일/음성 공유의 실제 OS 전달, 실제 음성 인식·OS 공유 메뉴·Push·키보드/스크롤·
   접근성을 지원 기기에서 검증한다.
5. **P8 볼트와 credential 사용**: 실제 native GitHub adapter·archive unlock·scope 재사용과
   명시적 요청 화면 및 원래 채팅의 대기/완료는 구현했다. 현재 live vault 조회·같은 범위의 credential 교체와 native Google INBOX 개수·별도 미리보기·선택한 일반 텍스트 본문 조회도 구현했다. 별도 연결의 최종 승인된 즉시/고정 예약 Gmail 발송·같은 기기에서의 불명 결과 확인을 구현했다. 별도 모델 읽기 grant의 Gmail 검색/thread/PDF·텍스트 읽기를 같은 대화·위임 업무에 연결했다. 일반 credential·Gitea 외 사이트 로그인/읽기·임의 쓰기 도구는 남는다.
   운영 중 암호화 snapshot/시점 복구도 구현했다. 명시적으로 고정한 이후 journal을 합치는 복구도 구현했다. 구 v1 암호화 파일의 로컬 이전을 구현했다. 주기적 SSH journal publisher/receiver도 구현했다. 실제 off-host 배치·무손실 최신 기록·실제 옛 파일 수락·서명 배포는 남는다. 계정 삭제 outbox는 구현·통합 검증했다.
6. **P9/P10 이전·출시**: 정규화 private importer/대조와 schema23 relation adapter를 구현했다. 실제 operator export·제한 cutover·rollback,
   공개한 Mac/Linux 네 플랫폼 설치 preview와 Linux x86_64 로컬 서명 설치/schema47부터 현재까지 update/제거·source-lock SBOM·서명된 출처를 별도 볼트/companion 공식 패키지·물리 ARM/Intel/WSL2/서비스 수락·독립 빌드 증명/전체 license 검토로 확장하고(Rust·웹 고지 수집기와 별도 출처 보충은 구현),
   깨끗한 host와 실제 기기/계정·독립 holdout/성능 gate를 완료한다.
   기존 운영은 검증된 전환 없이 변경하지 않는다.

F43 차량/지리/Todoist/local STT engine, PostgreSQL·공개 SaaS·private GitHub auth 등은 명시적 후속이다.
F14의 bounded STT는 구현했고 실제 provider/device 검증은 남는다. 자동 구매/웹 예약·전화 발신·Slack 쓰기·reply-all/첨부 메일 발송·
미래 생성 본문의 자동 발송·임의 shell 설치는 현재 기본 계약이 아니다. [기능표](features.md)의 제외/후속을 따른다.

## 검증 기록을 읽는 기준

날짜별 기록은 해당 frozen 소스·합성/provider/device 범위의 증거다. 서로 겹치는 필터 수를 합산해
전체 고유 시험 수나 assistant 품질 점수로 만들지 않는다. 이 문서 정리는 제품 시험을 다시 실행하지 않았다.
실제 모바일 viewport 확인과 iOS/Android 실기기, loopback provider와 실계정 실행을 구분한다.

기존 HTTP 자료 조회 fixture가 현재 일정 분류 단계를 처리하지 못하던 문제는
[시험 응답 수정](reviews/2026-11-14-knowledge-http-fixture.md)으로 해결했다. 실제 제품
분류 동작을 바꾸지 않았으며 삭제·늦은 응답 차단과 호출별 사용량 검사를 유지했다.

[Live ChatGPT 개발 평가](reviews/2026-11-13-live-chatgpt-quality.md)는 기존 공식 로그인으로
반복 개발 corpus36/36 intent·end-to-end7/10을 기록했다. 이후
[대상 해석 수정](reviews/2026-11-13-memory-target-resolution.md)은 합성 검사이며 새12-case live holdout은
연결 probe 실패 뒤 미실행이다. 이는 출시된 구독 adapter나 전체 비서 품질 성적이 아니다.
독립 비교/복구·crypto·기기/릴리스 gate의 전체 완료를 주장하지 않는다.

과거 단위별 경과·시험 수·수정 내용은 [정리 전 누적 기록](reviews/2026-11-13-status-history.md)에 보존했다.
새 세션은 README → 이 문서 → 필요한 영역 문서/날짜별 증거 순서로 읽는다.

새 [실모델 개발 평가](reviews/2026-11-15-live-agent-quality.md)에서 GPT-6.1 Terra/high의
기억 명령36/36과 무호출 분기5/5를 확인했다. 별도로 검증한 공식 이력 재입력 연결로
실제 앱11개 시나리오를 실행했고, 독립 의미 검토는9개 성공·삭제1개 실패·그에 따른
회상1개 종속 실패였다. 학습한 계산 절차의 새 대화·위임 업무 재사용도 실제 영수증으로
확인했다. 최초 실패·시간 초과·채점기 오판과 후속 수정/진단은 같은 보고서에서 분리한다.
[기존 직접 연결의 빈 답변](reviews/2026-11-14-live-quality-probe.md)은 해결로 표시하지
않으며 이 결과를 운영 구독 adapter·독립 holdout·전체 비서 품질 수락으로 확대하지 않는다.
