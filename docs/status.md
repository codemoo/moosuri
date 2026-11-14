# 현재 상태

2026-11-14. 실제 지원 범위·검증 한계·남은 작업은 이 문서가 정본이다.
[기능표](features.md)는 요구 계약, [계획](plan.md)·[이전 순서](migration.md)는
전체 단계, 아래 날짜별 기록은 해당 소스의 증거다. 공개 개발 저장소는
[codemoo/moosuri](https://github.com/codemoo/moosuri)이며 안정판·운영 전환은 아직 아니다.
GitHub Actions/자동 CI는 사용자 지시로 꺼져 있다. 로컬 검증을 작업 단위별로 수행한다.

## 현재 지원하는 개발 프리뷰

| 영역 | 구현한 범위 | 남은 경계와 사용·증거 |
| --- | --- | --- |
| 설치·계정 | Rust CLI/API·내장 React·SQLite single writer, 최초 admin, 로컬 초대·임시 암호/강제 변경, 세션 유지·철회, 계정 정지/재개·역할·암호 재발급 | 초대 메일/gws·전체 계정 삭제/볼트 outbox·일반 installer/update/uninstall은 미구현. [개발](development.md)·[계정](account-management.md)·[검증](reviews/2026-11-13-account-management.md) |
| 모델·대화 | 공유 OpenAI/Anthropic API·OpenRouter·compatible/local 설정, 실제 text/tools/image 연결 확인, model/effort pin, 사용량, 저장 채팅·SSE·취소·재시도·초안 복구·격리, 같은 컴퓨터의 개인 ChatGPT 로그인·sealed session·text proof/선택 | 원격 VM handoff·개인 tools/media/background·Claude 실행·실계정 및 종합 실모델 품질 검증은 미완료. 모델 이름으로 capability를 추정하지 않는다. [모델](provider-settings.md)·[전송](model-transport.md)·[plan protocol 검증](reviews/2026-11-14-chatgpt-plan-protocol.md)·[대화](chat.md) |
| 기억·자료 | 사용자별 원문/근거·검색, 직접 기억·정정·망각, opt-in literal 자료 추출·예산·망각 억제, 문서/목표/요청 편집·내보내기 | 넓은 대상 해석·추출 정확도·장기 문맥/시간 사실의 독립 평가와 자동화 전체 완료는 아니다. [지식](knowledge.md)·[기억](conversational-memory.md)·[추출](automatic-memory.md)·[문서](artifacts.md) |
| 도구·스킬 | 검증된 bounded tool loop, 여섯 내장 절차·정확한 소수 계산, 개인 curated/public GitHub pin·update/rollback/disable/remove, 로컬 작성/check·정적 catalog 생성 | 성공 절차의 typed 학습/독립 proof·합성 재구성 export·catalog runtime consumer·예약 needs_skill/GC 등은 남음. [업무](task-skills.md)·[community](community-skills.md)·[검증](reviews/2026-11-13-community-authoring.md) |
| 웹·브라우저·메일 | 공개 HTTP/Brave·검증된 Linux Chromium 읽기, 대화별 workspace·화면 보기·사람의 로그인/조작·같은 페이지 후속 대화, 개인 Gmail 읽기/thread·로컬 답장 초안·승인된 frozen 발송·불명 결과의 제한된 보낸편지함 조회 | 메일은 매번 최종 수신인/본문 등을 보고 명시적으로 허락해야 한다. Gmail provider 초안 CRUD·고정 예약 발송, 인증된 자동 폼/장기 profile 복구는 미완료. [웹](web-research.md)·[브라우저](browser-workspaces.md)·[메일](mail.md) |
| 공식·custom 자료 연결 | 개인 Gmail/Slack 수집·custom ingest, scope/cursor/receipt/provenance·상태 화면·철회 | 합성 Linux 검증은 실제 OAuth/계정 scope/rate 검증을 대신하지 않는다. 카톡 코드는 공개 패키지에서 제외. 통화·작업 세션·배송 adapter는 남음. [연결](data-connections.md)·[검증](reviews/2026-11-13-source-connections.md) |
| 알림·소식·Push | 한글/영문 제한 고정 알림 capture/control, 일회/간격/매일/매주·IANA/DST·latest-only 회차, inbox·quiet hours, opt-in 기기 Push | topic mute·전체 proactive 제안 정책·실제 휴대폰 설치/수신/백그라운드는 미완료. [예약](schedules.md)·[Push](web-push.md)·[검증](reviews/2026-11-13-web-push.md) |
| 맡긴 일 | 양식/API와 현재 대화의 명시적 위임으로 일회성·반복 읽기/계산/공개 HTTP 업무 등록·제어, 실제 영수증과 좁은 정확 일치 알림 생략, 의미 조건은 검토 필요 | 여러 턴의 자동 명확화·넓은 자연어/시간 해석·Calendar/mail/browser/vault 미션 실행은 미완료. [맡긴 일](missions.md)·[일회성 검증](reviews/2026-11-13-owner-missions.md)·[반복 검증](reviews/2026-11-13-recurring-missions.md)·[대화 검증](reviews/2026-11-13-conversational-missions.md) |
| Calendar | 별도 개인 동의, 대화/직접 양식의 기본 캘린더 읽기·명확한 단일 timed solo-event 생성/수정/삭제, ETag/dispatch receipt·불명 결과 lookup | 여러 턴 대상 해석·날짜 간 이동·미션 Calendar·반복/종일/참석자/보조 캘린더 쓰기·실계정 검증은 미완료. [Calendar](calendar.md)·[대화 검증](reviews/2026-11-13-conversational-calendar.md) |
| 미디어·PWA | 독립 image/UTF-8/PDF 텍스트 upload·preview/library·삭제 철회, 실제 vision probe, 녹음·미리 듣기·별도 OpenAI STT·수정 후 초안 삽입, 공개 offline shell·설치/업데이트 | 실제 음성 인식 품질·PDF OCR/시각 이해·PWA 공유받기·실기기는 미완료. 첨부 문맥은 도구/지속 쓰기를 제한한다. [이미지](media.md)·[파일](private-files.md)·[PWA](pwa.md)·[파일 검증](reviews/2026-11-13-private-files.md)·[PDF 검증](reviews/2026-11-14-private-pdf-files.md) |
| 독립 볼트 | 별도 Rust 암호문 서비스/DB/origin/session·WASM Worker·React, 항목/TOTP/로컬 CSV·키 교체, 독립 암호화 파일 export/import·봉인된 현재 service checkpoint 복구 | ongoing/off-host 최신 journal·과거 service 복구·구 v1 이전·account deletion/outbox·CredentialBroker·서명 client·배포/실기기는 남음. served JS의 악성 서버 한계는 유지. [볼트](vault-service-client.md)·[archive](vault-archive.md)·[archive 검증](reviews/2026-11-13-vault-archive.md)·[service 복구 검증](reviews/2026-11-13-vault-service-recovery.md) |
| 복구 | fsynced auth/삭제 journal, stopped age export·recipient key epoch, 봉인된 현재 checkpoint의 새 root 복원·격리, SQLite 이미지/파일/문서 보존 | ongoing/off-host 최신 journal·과거 export replay·unattended backup·실운영 이전은 미구현. 원본을 영구 봉인하는 cutover와 일반 재해복구를 구분한다. [backup](backup.md)·[restore](restore.md)·[검증](reviews/2026-11-13-sealed-restore.md) |

## 최근 수락한 단위 · 녹음과 초안 입력

녹음·일시 정지·미리 듣기와 별도 OpenAI API의 제한된 음성 변환을 연결했다.
처리 중에도 글을 보낼 수 있고, 결과는 고친 뒤 원래 초안에 한 번 넣는다. 삽입 후 패널을
접고 입력창으로 돌아온다. 응답이 끊겨도 원래 작업을 조회해 중복 비용을 방지하며,
연결을 끈 뒤 같은 연결을 다시 켜도 이전 글이 되살아나지 않는다.

독립 검토·Native/Linux·실제 Chrome의 녹음기/변환 워커/채팅 검사를 통과했다.
브라우저 입력과 음성 제공업체 응답은 합성이므로 한국어 인식 품질이나 실제 모바일
마이크 검증은 아니다. [검증](reviews/2026-11-14-bounded-voice-input.md).

## 최근 수락한 단위 · 같은 컴퓨터의 개인 ChatGPT 연결

공식 로그인 helper·개인 credential 저장/갱신·정확한 model/effort text 확인·선택·저장 대화를
연결했다. 토큰 갱신 후 오류의 재전송과 이전 첨부의 의도치 않은 전송을 독립 검토로
재현·수정했다. Native/Linux·React·실제 Rust 서버의 설정/대화 브라우저 검사를 통과했다.
현재는 같은 컴퓨터의 text 범위이며 원격 Linux/VM 전달과 개인 도구·media·background는
이어질 구현이다. 실계정 로그인 수락은 아니다. [검증](reviews/2026-11-14-personal-chatgpt.md).

## 최근 수락한 단위 · 저장된 기억 작업 재시도

재시도 중 이미 확정한 기억 작업이 다른 분류 단계에 막혀 실패하던 실행 순서를 수정했다.
같은 저장된 작업을 현재 권한으로 이어서 완료하며 모델에 새 계획을 요청하지 않는다.
기존 실패 테스트를 바꾸지 않고 Linux 에이전트·저장소 검사와 독립 재검토를 통과했다.
[검증 기록](reviews/2026-11-14-sealed-memory-retry.md).

## 최근 수락한 단위 · 공개 스킬 catalog 생성

정확한 공개 GitHub commit과 package 경로에서 검증한 스킬 파일을 가져와 재현 가능한
정적 catalog와 설치용 artifact를 만든다. 개인 자료나 로컬 미커밋 파일을 읽지 않는다.
출력 경로의 다른 사용자 변조 가능성과 macOS의 빈 ACL 처리 결함을 독립 검토로 발견·
수정하고 macOS/Linux 실행 검사를 통과했다. 설치는 기존 GitHub 선택 화면을 사용하며
자동 catalog 소비·활성화는 아직 없다. [검증](reviews/2026-11-14-community-catalog.md).

## 최근 수락한 단위 · 대화 초안과 재접속

같은 탭에서 새로고침하거나 오프라인 뒤 다시 연결해도, 정확한 로그인 세션과 대화 목록을
확인한 뒤 작성 중인 글과 미확인 요청을 복구한다. 자동으로 메시지를 다시 보내지 않으며
직접 재제출할 때 원래 요청을 유지한다. 로그아웃·다른 세션에서는 이전 초안을 지운다.
재접속이 초안을 지우던 결함을 독립 재현·수정하고 실제 Chrome/Rust/SQLite에서 검증했다.
탭의 제한된 평문 보관이며 볼트 암호화나 탭 종료 후 백업은 아니다.
[검증 기록](reviews/2026-11-14-chat-draft-continuity.md).

## 최근 수락한 단위 · 메일 결과 확인

발송 응답이 끊긴 메일은 최대 네 번의 읽기 전용 조회와 사용자의 직접 확인을 지원한다.
원래 내용과 일치한 관측은 발송 응답·상대 수신 확인과 구별하며 자동 재발송하지 않는다.
무효 항목이 다른 사용자의 조회를 계속 막던 결함을 수정하고 독립 재검토·Linux·실제 웹
화면 검증을 통과했다. Google이 변경한 메일 형식은 엄격한 비교를 통과하지 못할 수 있고,
실제 Gmail의 비교 성공률은 아직 검증하지 않았다. [검증 기록](reviews/2026-11-14-gmail-reconciliation.md).

## 최근 수락한 단위 · 대화에서 Calendar 읽기와 일정 작업

현재 사용자의 명확한 요청으로 기본 캘린더의 단일 시간 지정 일정을 만들고, 같은 날짜의
제목/시간을 바꾸거나 삭제할 수 있다. 모호한 대상·종료 시각은 변경하지 않고 물어본다.
기존 typed CalendarService를 공유하며 추가 승인 창 없이 실제 작업 영수증을 표시한다.
결과가 불명확하면 조회만 하고 새 작업으로 재발송하지 않는다.

독립 검토에서 취소→재시도 뒤 늦은 응답이 private 일정 정보를 되살리는 결함을 재현하고,
되돌릴 수 없는 철회 표시와 원자적 데이터 제거로 수정했다. Native/Linux 저장소·실제 typed
IO·HTTP, 실제 Rust/SQLite와 Chrome의 명확화/설정 링크/영수증 재조회가 통과했다.
[검증 기록](reviews/2026-11-13-conversational-calendar.md)에 수정과 수락 범위를 남겼다.

응답 중 작성한 초안과 같은 탭의 검증된 새로고침 복구를 지원한다.
실제 Google 계정·모델의 언어 이해 정확도·실기기는 별도 검증이다. 앞서 수락한
[PDF 텍스트 첨부](reviews/2026-11-14-private-pdf-files.md)와
[독립 볼트 현재 checkpoint 복구](reviews/2026-11-13-vault-service-recovery.md)는 유지한다.

ChatGPT 구독의 별도 catalog·요청 형식·namespace 도구 protocol도 검증했다. 잘못된 response
identity의 실패 사용량이 섞이는 결함을 수정하고 독립 재검토·Native/Linux 검사를 통과했다.
[기록](reviews/2026-11-14-chatgpt-plan-protocol.md)의 범위는 protocol이며 로그인/구독 선택을
아직 활성화하지 않는다. 이어 [OAuth protocol](reviews/2026-11-14-chatgpt-authentication.md)의
서명 검증·재로그인·refresh/revoke도 독립 검토와 Native/Linux 검사를 통과했다. 이어 개인 저장·
같은 컴퓨터의 로그인 helper·text capability/선택·대화를 연결했다. 원격 VM 전달과 개인
도구·media·background 및 실계정 수락은 아직 남는다.

## 다음 구현과 출시까지 남은 일

우선순위는 현재 지원 범위를 실제 대화 업무에 연결하는 순서다. `선택` 연결/볼트 패키지는
사용자의 설치 선택이며 v1 제공 계약을 후속으로 미루는 뜻이 아니다.

1. **대화와 Calendar 후속**: 여러 턴의 대상 명확화,
   현재 범위의 실계정·언어 holdout 검증을 진행한다. 기본 캘린더의 제한된 대화 읽기/쓰기는
   구현했고, 미션·반복/종일/참석자/보조 캘린더 작업은 아직 확장하지 않았다.
2. **볼트 broker와 복구**: device pairing·local grant·online commitment/revoke·secret 비노출·offline resume,
   완료한 standalone 현재 checkpoint 복구에 이어 ongoing/off-host 복구·구 v1 호환/이전,
   전체 계정 삭제/outbox를 작은 경계별 단위로 구현한다.
3. **미디어·외부 연결**: one-use share와 실제 음성/기기 검증; Gmail provider draft·승인된 고정 예약 발송,
   통화 feed·opt-in 작업 세션·공식 배송 parser를 각각 receipt/실패 계약과 구현한다.
4. **학습·품질·소식**: typed 성공 절차와 독립2입력 proof, 합성 export/catalog·예약 lifecycle,
   대화의 여러 턴 명확화·넓은 시간 해석, followup/topic mute/알림 정책,
   pinned baseline/독립 grader·unseen holdout·latency/usage 분모를 보완한다.
5. **연결·운영·출시**: 실제 공식 지원을 확인한 구독/개인 provider 연결, gws 초대,
   ongoing 최신 off-host journal/과거 복구, signed Linux x86_64/arm64 설치·update/drain/uninstall·SBOM,
   합성 importer/제한 cutover·깨끗한 host와 실제 기기/계정 gate를 완료한다. 기존 운영은 검증 전환 없이 변경하지 않는다.

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
