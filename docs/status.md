# 현재 상태

2026-11-15. 실제 지원 범위·검증 한계·남은 작업은 이 문서가 정본이다.
[기능표](features.md)는 요구 계약, [계획](plan.md)·[이전 순서](migration.md)는
전체 단계, 아래 날짜별 기록은 해당 소스의 증거다. 공개 개발 저장소는
[codemoo/moosuri](https://github.com/codemoo/moosuri)이며 안정판·운영 전환은 아직 아니다.
GitHub Actions/자동 CI는 사용자 지시로 꺼져 있다. 로컬 검증을 작업 단위별로 수행한다.
일반 질문의 “부탁해” 표현만으로 추가하던 업무 분류 모델 호출을 제거했다. 해당 경로의
호출 수와 기존 예약 해석을 Linux 실제 Runtime에서 확인했으며 실모델 속도 비율을 주장하지 않는다.
[호출 감소 검증](reviews/2026-11-13-conversational-missions.md#2026-11-14-ordinary-polite-request-routing).

## 현재 지원하는 개발 프리뷰

| 영역 | 구현한 범위 | 남은 경계와 사용·증거 |
| --- | --- | --- |
| 설치·계정 | Rust CLI/API·내장 React·SQLite single writer, 최초 admin, 로컬 초대·임시 암호/강제 변경, 세션 유지·철회, 계정 정지/재개·역할·암호 재발급 | 전체 계정 삭제·개인 자료 정리·볼트 삭제 outbox/ACK를 구현했다. 최종 승인 후 gws 초대 메일·로컬 서명 bundle 설치/schema47부터 현재까지 stopped update/데이터 보존 제거도 구현했다. 공식 release·실계정 수락은 남음. [개발](development.md)·[계정](account-management.md)·[검증](reviews/2026-11-13-account-management.md) |
| 모델·대화 | 공유 OpenAI/Anthropic API·OpenRouter·compatible/local 설정, 실제 text/tools/image 연결 확인, model/effort pin, 사용량·실제 실행 단계 시간/누락 집계, 저장 채팅·SSE·취소·재시도·초안 복구·격리, 오래된/다른 소유 대화의 제한 검색·읽기, 개인 ChatGPT 로그인·sealed session·text/tool proof·선택·native Linux 전달·foreground 도구·기억/자료·제한된 Calendar 변경·명시적 일회/반복 읽기 미션 | 개인 Claude 공식 CLI의 설정·text probe·선택·저장 대화도 구현했다. Claude 실제 rootless Linux/공식 로그인과 media/background·실계정 및 종합 실모델 품질 검증은 미완료. 모델 이름으로 capability를 추정하지 않는다. [모델](provider-settings.md)·[전송](model-transport.md)·[plan protocol 검증](reviews/2026-11-14-chatgpt-plan-protocol.md)·[대화](chat.md) |
| 기억·자료 | 사용자별 원문/근거·기록 시각별 검색, 직접 기억·정정·망각·대화의 제한된 절대 날짜 적용, opt-in literal 자료 추출·예산·망각 억제, 문서/목표/요청 편집·내보내기 | 넓은 대상 해석·추출 정확도·장기 문맥/시간 사실의 독립 평가와 자동화 전체 완료는 아니다. [지식](knowledge.md)·[기억](conversational-memory.md)·[추출](automatic-memory.md)·[문서](artifacts.md) |
| 도구·스킬 | 검증된 bounded tool loop, 내장 절차·정확한 소수 계산, 개인 curated/public GitHub pin·update/rollback/disable/remove, 로컬 작성/check·정적 catalog 생성·설정한 공개 목록의 발견/원본 확인 | 정확 계산·저장 자료 조회·일본우편 최근 요약의 개인 학습/두 입력 proof·새 입력 재사용은 구현. 알려진 절차의 합성 공유 파일 내보내기도 구현. 실제 사용한 스킬의 예약 정지/복구와 참조 없는 공개 snapshot 정리를 구현했다. 2–4단계 정확 계산과 유일한 저장 자료의 숫자를 읽는 근거 계산의 구조 학습·새 입력 재사용·합성 export도 구현했다. 일반 외부 복합 업무 학습/export·전체 이력 보존/정리 정책은 남음. 공개 목록은 운영자가 설정하며 자동 설치하지 않는다. [업무](task-skills.md)·[community](community-skills.md)·[검증](reviews/2026-11-14-community-catalog-runtime.md) |
| 웹·브라우저·메일 | 공개 HTTP/Brave·검증된 Linux Chromium 읽기, 대화별 workspace·화면 보기·사람의 로그인/조작·같은 페이지 후속 대화, 개인 Gmail 읽기/thread·로컬/Google 초안 CRUD·승인된 즉시/고정 예약 발송·불명 결과의 제한된 보낸편지함 조회 | 메일은 매번 최종 수신인/본문 등을 보고 명시적으로 허락해야 한다. 인증된 자동 폼/장기 profile 복구·실제 Gmail 계정 검증은 미완료. [웹](web-research.md)·[브라우저](browser-workspaces.md)·[메일](mail.md) |
| 공식·custom 자료 연결 | 개인 Gmail/Slack 수집·custom ingest, 선택한 Codex 사용자 요청·정규화 통화 전사의 일회 수집, scope/cursor/receipt/provenance·상태 화면·철회, 일본우편 최근 상태 조회 | 합성 Linux 검증은 실제 OAuth/계정 scope/rate 검증을 대신하지 않는다. 카톡 코드는 공개 패키지에서 제외. schema48에서 시작 ACK 유실 복구를 추가했다. 통화 원본 기기/포맷·실제 전사 품질·다른 작업 세션 형식·다른 배송사/전체 이력은 남음. [연결](data-connections.md)·[검증](reviews/2026-11-13-source-connections.md) |
| 알림·소식·Push | 한글/영문 제한 고정 알림 capture/control, 일회/간격/매일/매주·IANA/DST·latest-only 회차, inbox·quiet hours·예약별 소식 끄기/다시 받기, 저장한 followup 마감/재확인/미룬 날짜 알림, opt-in 기기 Push | 전체 proactive 제안 정책·실제 휴대폰 설치/수신/백그라운드는 미완료. [예약](schedules.md)·[Push](web-push.md)·[소식 제어](reviews/2026-11-14-notification-topics.md) |
| 맡긴 일 | 양식/API와 현재 대화의 명시적 위임으로 일회성·반복 읽기/계산/공개 HTTP·명시한 개인 Gmail/기본 Calendar 읽기 업무 등록·제어, 실제 영수증과 좁은 정확 일치 알림 생략, 의미 조건은 검토 필요 | 빠진 값을 다음 답변으로 보완하고 직전 업무 영수증으로 제어한다. 정지한 일회성 업무는 기존 한도 안에서 조건을 고쳐 다시 계획할 수 있다. 넓은 자연어/시간 해석·메일/일정 쓰기·browser/vault 미션 실행·Claude background는 미완료. 개인 ChatGPT의 명시적 읽기 미션은 지원한다. [맡긴 일](missions.md)·[일회성 검증](reviews/2026-11-13-owner-missions.md)·[반복 검증](reviews/2026-11-13-recurring-missions.md)·[대화 검증](reviews/2026-11-13-conversational-missions.md) |
| Calendar | 별도 개인 동의, 대화/직접 양식의 기본 캘린더 읽기·명확한 단일 timed/종일 solo-event 생성/수정/삭제, ETag/dispatch receipt·불명 결과 lookup | 확인 질문의 다음 답변·직전 일정 지칭·기간을 유지한 날짜 이동을 지원한다. 반복/참석자/보조 캘린더 쓰기·실계정 검증은 미완료. [Calendar](calendar.md)·[대화 검증](reviews/2026-11-13-conversational-calendar.md) |
| 미디어·PWA | 독립 image/UTF-8/PDF 텍스트 upload·preview/library·삭제 철회, 실제 vision probe, 녹음·미리 듣기·별도 OpenAI STT·수정 후 초안 삽입, PWA 글/링크/사진/파일/음성 공유받기·공개 offline shell·설치/업데이트 | 실제 음성 인식 품질·PDF OCR/시각 이해·휴대폰 OS 공유 메뉴·실기기는 미완료. 첨부 문맥은 도구/지속 쓰기를 제한한다. [이미지](media.md)·[파일](private-files.md)·[PWA](pwa.md)·[파일 검증](reviews/2026-11-13-private-files.md)·[PDF 검증](reviews/2026-11-14-private-pdf-files.md) |
| 독립 볼트 | 별도 Rust 암호문 서비스/DB/origin/session·WASM Worker·React, 항목/TOTP/로컬 CSV·키 교체, 독립 암호화 파일 export/import·봉인된 현재 service checkpoint·운영 중 암호화 snapshot/시점 복구·실험적 broker 기기/서명/철회 기반 | 실험적 native companion이 현재 독립 vault 또는 명시한 암호화 archive의 선택 credential로 정확한 GitHub 저장소를 읽고 명시적 채팅 요청의 같은 실행에 결과를 돌려준다. 주기적 pinned SSH journal receiver를 구현했다. 기기 내 Google INBOX 개수와 별도로 선택한 최대5개 메일 미리보기도 구현했다. 일반 credential/Google 본문·쓰기 도구·실제 off-host 배치·서명 client·실기기는 남음. 구 v1 암호화 파일의 기기 내 이전을 구현했다. served JS의 악성 서버 한계는 유지. [볼트](vault-service-client.md)·[archive](vault-archive.md)·[archive 검증](reviews/2026-11-13-vault-archive.md)·[service 복구 검증](reviews/2026-11-13-vault-service-recovery.md) |
| 복구 | fsynced auth/삭제 journal, stopped age export·recipient key epoch, 봉인된 현재 checkpoint와 수동 최신 journal을 고정한 과거 export 복원·격리, SQLite 이미지/파일/문서 보존 | 과거 사용량 불명을 유지하면서 운영자가 새 호출·음성 길이 한도를 부여할 수 있다. 주기적 live journal·고정 SSH receiver와 실행 중 전체 자료의 암호화된 로컬 export를 구현했다. 실제 off-host 배치·무손실 최신 삭제·live 전체 DB export의 실제 운영 수락·실운영 이전은 남음. 원본을 영구 봉인하는 cutover와 일반 재해복구를 구분한다. [backup](backup.md)·[restore](restore.md)·[검증](reviews/2026-11-13-sealed-restore.md) |

## 이번에 반영한 기능

| 단위 | 반영 내용과 범위 | 증거 |
| --- | --- | --- |
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
| P4 도구·학습 | typed 도구·bounded 실행·Git 스킬·제안·연결 Gmail/Calendar 읽기 미션·예약 스킬 의존성/정리·제한된 개인 학습/두 입력 proof/재사용 | 일반 외부 복합 절차·범용 학습 export·실모델 효용/품질 검증 |
| P5 예약·소식 | due loop·반복 회차·알림·빠진 값의 다음 답변 보완·quiet hours·예약별 소식 제어·followup 날짜 알림·Push | 전체 알림/제안 정책·실기기 수신·고정 메일 외 예약 외부 작업 |
| P6 외부 연결 | Gmail/Calendar·공식/커스텀 수집·선택한 Codex 요청·정규화 통화 전사 수집·공개 웹·일본우편 최근 상태·브라우저·승인된 gws 초대 | 통화 vendor 포맷/실제 전사·다른 작업 feed·다른 배송사/전체 이력·실계정 |
| P7 첨부·모바일 | 이미지/파일/PDF 텍스트·녹음·글/링크/파일/음성 공유 | 모바일/접근성·실기기 |
| P8 독립 볼트 | 암호문 서비스·클라이언트·CSV/TOTP·현재 복구·운영 중 snapshot/시점·명시적 최신 journal 복구·주기적 SSH journal receiver·구 v1 파일 이전·실험적 native GitHub/Google INBOX 개수·미리보기 조회·원래 채팅 대기/완료 | 일반 credential/Google 본문·쓰기 도구·실제 옛 파일 수락·실제 off-host/무손실 최신 복구 |
| P9 이전·전환 | 정규화한 private bundle의 새 계정·원문/기억/문서 이력 이전·대조/격리 보고·합성 복구 | 실제 operator export·운영 대조·drain/cutover/rollback 실증 |
| P10 출시 | 개발 프리뷰·공개 문서·key-pinned 로컬 서명 설치/schema47부터 현재까지 update/제거·CycloneDX source-lock inventory·서명된 binary/source/compiler/inventory 연결·ARM64 교차 빌드/직접 에뮬레이션 | 공식 서명 배포·물리 ARM/설치 경로·독립 빌드 증명/license·품질/기기/출시 gate |

수락한 Calendar 단위는 P6으로, broker 기반 단위는 P8으로 추적한다.
P0 합성 결과 실행기·독립 판정기와 두 세계 비교 adapter를 구현했다. 기존 비서의 실제 비교 실행은
호스트 공유 메모리 부족으로 미완료이며 넓은 품질 평가도 남는다.
P4의 정확 계산·저장 자료 조회·일본우편 최근 요약과2–4단계 계산 그래프, 유일한 원문 숫자에 묶은1–4단계 계산의 학습·새 입력 재사용·합성 공유를 수락했다. 일반 외부 복합 업무·실모델
검증은 남은 계약으로 유지한다. 새로운 내부 작업명을 제품 계획의 P 번호로 사용하지 않는다.
CI는 사용자 지시에 따라 계속 비활성이다.

## 다음 구현과 출시까지 남은 일

진행 중인 단위는 위 표의 단계에 연결하고, 다음 미완료 계약을 기존 계획의 의존순서대로
닫는다. `선택` 연결/볼트 패키지도 v1 제공 대상이며 사용자의 설치 선택을 뜻한다.

1. **P0 평가 기준·P1/P2 기초의 남은 계약**: 기존 소스의 내용 일치 검사는 구현했다.
   이를 고정한 로컬 사본·두 공통 fixture adapter와 실행 설정을 연결했다. 실제 비교는
   Mac의 공유 메모리 슬롯32개가 이미 사용 중이라2건 모두 미실행이다. 다른 자원이나
   시스템 설정을 변경하지 않았으며, 환경이 준비된 뒤 같은 고정 비교를 실행해야 한다.
   초기 독립 상태/영수증 grader와 실패를 포함한 관측
   latency/usage 집계와 실제 서버 단계별 계측은 구현했다. 넓은 사례·실모델 평가와
   브라우저 표시 시간·성능 개선은 별도로 이어간다.
   수동 과거 복원과 명시적 새 사용 한도를 수락했다.
   주기적 live journal/독립 SSH receiver와 실행 중 전체 자료의 암호화된 로컬 export를 구현했다. 실제 off-host 배치·무손실 최신 삭제와
   지원 구독 실행 범위는 해당 저장/연결 의존성과 함께 닫는다.
2. **P3/P4 기억·학습·업무 수행**: 초기 합성 평가와 계산/자료 조회의 typed 학습·
   두 입력 proof와 대화 기억/전이 망각/업무 취소/학습 재사용의 실제 결과 평가를 구현했다.
   명시적 날짜의 대화 기억·정정과 현재/과거 조회 및 원래 시간대 복구를 구현했다.
   자료의 기록 시각 범위를 API·읽기 도구·검색 화면에 연결했다.
   넓은 시간 표현·실모델 이해력·복합 외부 절차의 독립 평가로 확장한다.
   정확 계산 그래프와 유일한 저장 원문 숫자에 묶은 계산의 합성 export는 구현했으며 일반 외부 복합 절차·전체 이력 보존/정리 정책은 남는다.
   정지한 일회성 업무의 기준 수정·기존 한도 내 재계획/검증/보완은 구현했다.
   제한된 여러 턴 명확화와 넓은 시간 해석·일반 복합 작업으로 이어간다.
3. **P5/P6 예약·외부 연결**: 전체 알림/제안 정책,
   구현한 Gmail provider draft·승인된 고정 예약 발송과 Calendar 후속 명확화의 실계정 검증,
   선택한 Codex 요청과 정규화 통화 전사 수집·schema48 시작 ACK 복구는 구현했다. 통화 vendor 포맷/실제 전사·다른 작업 형식(Claude 원본 로그의 사람/자동화 구분은 [공식 형식의 한계](local-work-sessions.md#claude-transcript-provenance-limit))·
   배송 전체 이력/다른 배송사와 실제 OAuth/계정 검증을 진행한다.
   공유 provider와 개인 ChatGPT의 명시한 Gmail/기본 Calendar 읽기 미션은 구현했다. 미션의 메일/일정 쓰기,
   반복/참석자/보조 캘린더 쓰기와 Claude background는 현재 지원 범위에 포함하지 않는다.
4. **P7 첨부·모바일**: 파일/음성 공유의 실제 OS 전달, 실제 음성 인식·OS 공유 메뉴·Push·키보드/스크롤·
   접근성을 지원 기기에서 검증한다.
5. **P8 볼트와 credential 사용**: 실제 native GitHub adapter·archive unlock·scope 재사용과
   명시적 요청 화면 및 원래 채팅의 대기/완료는 구현했다. 현재 live vault 조회·같은 범위의 credential 교체와 native Google INBOX 개수·별도 미리보기 조회도 구현했다. 일반 credential/Google 본문·쓰기 도구는 남는다.
   운영 중 암호화 snapshot/시점 복구도 구현했다. 명시적으로 고정한 이후 journal을 합치는 복구도 구현했다. 구 v1 암호화 파일의 로컬 이전을 구현했다. 주기적 SSH journal publisher/receiver도 구현했다. 실제 off-host 배치·무손실 최신 기록·실제 옛 파일 수락·서명 배포는 남는다. 계정 삭제 outbox는 구현·통합 검증했다.
6. **P9/P10 이전·출시**: 정규화 private importer/대조와 schema23 relation adapter를 구현했다. 실제 operator export·제한 cutover·rollback,
   수락한 Linux x86_64 로컬 서명 설치/schema47부터 현재까지 update/제거·source-lock SBOM·서명된 출처 연결을 공식 artifact·물리 ARM/설치 경로·독립 빌드 증명/완전한 license 검토로 확장하고(Rust·웹 고지 수집기와 별도 출처 보충은 구현),
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

새 [실모델 평가 연결 진단](reviews/2026-11-14-live-quality-probe.md)에서는 공식 로그인으로 텍스트 probe가 통과했으나 도구 결과 후 최종 답이 빈 상태가 두 번 관측됐다. 도구 capability를 우회하지 않았으며 새11개 의미 시나리오는 `not_run`이다. 기존 성적을 새 검증 결과로 재사용하지 않는다.
