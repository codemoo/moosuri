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
| 모델·대화 | 공유 OpenAI/Anthropic API·OpenRouter·compatible/local 설정, 실제 text/tools/image 연결 확인, model/effort pin, 사용량, 저장 채팅·SSE·취소·재시도·초안 복구·격리, 개인 ChatGPT 로그인·sealed session·text/tool proof·선택·native Linux 전달·foreground 도구·기억/자료·제한된 Calendar 변경 | media/background·Claude 실행·실계정 및 종합 실모델 품질 검증은 미완료. 모델 이름으로 capability를 추정하지 않는다. [모델](provider-settings.md)·[전송](model-transport.md)·[plan protocol 검증](reviews/2026-11-14-chatgpt-plan-protocol.md)·[대화](chat.md) |
| 기억·자료 | 사용자별 원문/근거·검색, 직접 기억·정정·망각, opt-in literal 자료 추출·예산·망각 억제, 문서/목표/요청 편집·내보내기 | 넓은 대상 해석·추출 정확도·장기 문맥/시간 사실의 독립 평가와 자동화 전체 완료는 아니다. [지식](knowledge.md)·[기억](conversational-memory.md)·[추출](automatic-memory.md)·[문서](artifacts.md) |
| 도구·스킬 | 검증된 bounded tool loop, 여섯 내장 절차·정확한 소수 계산, 개인 curated/public GitHub pin·update/rollback/disable/remove, 로컬 작성/check·정적 catalog 생성·설정한 공개 목록의 발견/원본 확인 | 성공 절차의 typed 학습/독립 proof·합성 재구성 export·예약 needs_skill/GC 등은 남음. 공개 목록은 운영자가 설정하며 자동 설치하지 않는다. [업무](task-skills.md)·[community](community-skills.md)·[검증](reviews/2026-11-14-community-catalog-runtime.md) |
| 웹·브라우저·메일 | 공개 HTTP/Brave·검증된 Linux Chromium 읽기, 대화별 workspace·화면 보기·사람의 로그인/조작·같은 페이지 후속 대화, 개인 Gmail 읽기/thread·로컬 답장 초안·승인된 frozen 발송·불명 결과의 제한된 보낸편지함 조회 | 메일은 매번 최종 수신인/본문 등을 보고 명시적으로 허락해야 한다. Gmail provider 초안 CRUD·고정 예약 발송, 인증된 자동 폼/장기 profile 복구는 미완료. [웹](web-research.md)·[브라우저](browser-workspaces.md)·[메일](mail.md) |
| 공식·custom 자료 연결 | 개인 Gmail/Slack 수집·custom ingest, scope/cursor/receipt/provenance·상태 화면·철회 | 합성 Linux 검증은 실제 OAuth/계정 scope/rate 검증을 대신하지 않는다. 카톡 코드는 공개 패키지에서 제외. 통화·작업 세션·배송 adapter는 남음. [연결](data-connections.md)·[검증](reviews/2026-11-13-source-connections.md) |
| 알림·소식·Push | 한글/영문 제한 고정 알림 capture/control, 일회/간격/매일/매주·IANA/DST·latest-only 회차, inbox·quiet hours, opt-in 기기 Push | topic mute·전체 proactive 제안 정책·실제 휴대폰 설치/수신/백그라운드는 미완료. [예약](schedules.md)·[Push](web-push.md)·[검증](reviews/2026-11-13-web-push.md) |
| 맡긴 일 | 양식/API와 현재 대화의 명시적 위임으로 일회성·반복 읽기/계산/공개 HTTP 업무 등록·제어, 실제 영수증과 좁은 정확 일치 알림 생략, 의미 조건은 검토 필요 | 여러 턴의 자동 명확화·넓은 자연어/시간 해석·Calendar/mail/browser/vault 미션 실행은 미완료. [맡긴 일](missions.md)·[일회성 검증](reviews/2026-11-13-owner-missions.md)·[반복 검증](reviews/2026-11-13-recurring-missions.md)·[대화 검증](reviews/2026-11-13-conversational-missions.md) |
| Calendar | 별도 개인 동의, 대화/직접 양식의 기본 캘린더 읽기·명확한 단일 timed solo-event 생성/수정/삭제, ETag/dispatch receipt·불명 결과 lookup | 여러 턴 대상 해석·날짜 간 이동·미션 Calendar·반복/종일/참석자/보조 캘린더 쓰기·실계정 검증은 미완료. [Calendar](calendar.md)·[대화 검증](reviews/2026-11-13-conversational-calendar.md) |
| 미디어·PWA | 독립 image/UTF-8/PDF 텍스트 upload·preview/library·삭제 철회, 실제 vision probe, 녹음·미리 듣기·별도 OpenAI STT·수정 후 초안 삽입, PWA 글/링크 공유받기·공개 offline shell·설치/업데이트 | 실제 음성 인식 품질·PDF OCR/시각 이해·파일/음성 공유·휴대폰 OS 공유 메뉴·실기기는 미완료. 첨부 문맥은 도구/지속 쓰기를 제한한다. [이미지](media.md)·[파일](private-files.md)·[PWA](pwa.md)·[파일 검증](reviews/2026-11-13-private-files.md)·[PDF 검증](reviews/2026-11-14-private-pdf-files.md) |
| 독립 볼트 | 별도 Rust 암호문 서비스/DB/origin/session·WASM Worker·React, 항목/TOTP/로컬 CSV·키 교체, 독립 암호화 파일 export/import·봉인된 현재 service checkpoint 복구·실험적 broker 기기/서명/철회 기반 | ongoing/off-host 최신 journal·과거 service 복구·구 v1 이전·account deletion/outbox·broker 실제 adapter/사용 흐름·서명 client·배포/실기기는 남음. served JS의 악성 서버 한계는 유지. [볼트](vault-service-client.md)·[archive](vault-archive.md)·[archive 검증](reviews/2026-11-13-vault-archive.md)·[service 복구 검증](reviews/2026-11-13-vault-service-recovery.md) |
| 복구 | fsynced auth/삭제 journal, stopped age export·recipient key epoch, 봉인된 현재 checkpoint의 새 root 복원·격리, SQLite 이미지/파일/문서 보존 | ongoing/off-host 최신 journal·과거 export replay·unattended backup·실운영 이전은 미구현. 원본을 영구 봉인하는 cutover와 일반 재해복구를 구분한다. [backup](backup.md)·[restore](restore.md)·[검증](reviews/2026-11-13-sealed-restore.md) |

## 최근 수락한 단위 · 볼트 credential broker 기반

기기 연결·서명 인증, 계정별 제한된 로컬 권한, 원래 요청의 실행 확정/철회/영수증,
프로세스 종료 후 중복 진입 방지와 복원 격리를 구현했다. 기본 빌드에서 HTTP 경로는
꺼져 있고 실제 vault 항목을 여는 adapter·잠금 대기/자동 재사용 화면은 아직 없다.
따라서 이 기반을 P8 전체나 실제 credential 사용 완료로 표시하지 않는다.

독립 검토·Native/Linux의 기기/서명·저장소·HTTP·실제 암호화 복원·Clippy·기본 빌드를
통과했다. Linux 시험용 복원 폴더의 권한을 명시해 host umask 의존도 제거했다.
[실제 범위](credential-broker-foundation.md)·[검증](reviews/2026-11-14-credential-broker-foundation.md).

## 최근 수락한 단위 · 개인 ChatGPT Calendar 작업

개인 ChatGPT에서도 현재 사용자의 명확한 요청으로 기본 캘린더의 단일 시간 지정 일정을
생성·같은 날짜 수정·삭제한다. 이미 저장한 작업은 새 모델 분류 없이 원래 영수증으로
이어간다. 개인 백그라운드 작업이나 반복/종일/참석자/보조 캘린더 쓰기는 포함하지 않는다.

연결이 없으면 설정으로 안내하며, 이 상태가 대화 전체를 깨뜨리던 웹 파서를 수정했다.
독립 검토에서 재개한 작업 도중 개인 연결을 끊으면 실행 함수가 종료 상태를 남기지 않는 경로를 Linux에서
재현했다. 미완료 작업 철회와 늦은 개인 결과 제거를 수정했다. 완료 기록 보존·실제 저장 데이터 제거·
재연결 후 중복 실행 방지를 검증하고 독립 검토, Linux 관련 저장소/전체 에이전트/HTTP·
Clippy·기본 빌드와 React 검사를 통과했다.
[검증 기록](reviews/2026-11-14-personal-chatgpt-calendar.md).

## 최근 수락한 단위 · 여러 연결을 함께 쓰는 대화

메일·웹·브라우저·기록·Calendar를 함께 연결했을 때 최대21개 도구가 기존16개 제한을
넘어 대화가 시작 전에 실패하던 문제를 수정했다. 정의 수만32개로 늘리고 요청 크기·
실행 횟수·현재 권한 검사는 유지한다. 기존 실패를 재현한 뒤 실제 plan/compatible HTTP,
32/33 경계·리눅스 프로토콜 회귀와 독립 검토를 통과했다.
[검증](reviews/2026-11-14-combined-tool-profiles.md).

## 최근 수락한 단위 · 개인 ChatGPT 기억과 기록 변경

현재 사용자의 명확한 요청으로 기억을 저장·정정·삭제하고, 저장 근거로 문서/목표/후속
기록을 만들거나 선택한 기록을 수정한다. 각 분류/대상 선택 전에 연결 상태를 확인하며,
실제 저장 영수증으로 완료를 표시한다. 이미 확정한 기억 작업은 새 모델 호출 없이
재시도한다. 기록을 쓰는 단계에서는 저장 자료 도구만 제공한다.

독립 검토, 합성 signed-plan의 실제 runner/Store 검사, 전체 React392와 통합 설정17,
리눅스 에이전트107/HTTP40·Clippy·기본 빌드를 통과했다. 이후 개인 Calendar 변경을 별도 수락했다. 백그라운드·
미디어, 실모델 언어 이해와 실계정 검증은 남는다.
[검증](reviews/2026-11-14-personal-chatgpt-writes.md).

## 최근 수락한 단위 · 개인 ChatGPT 도구 실행

개인 연결의 정확한 model/effort에서 별도 도구 확인을 마친 뒤 저장 자료 읽기·계산과
기존 연결 도구를 사용할 수 있다. 이전에 시작한 글 전용 대화의 권한을 뒤늦게 늘리지
않으며, 각 모델 호출 전에 현재 연결과 토큰 상태를 확인한다. 실제 메일은 계속 최종
미리보기 승인이 필요하다.

독립 검토·React391·리눅스 에이전트/HTTP/프로토콜 검사를 통과했다. 복구 시험의 이전
형식 데이터를 수정하고 실제 격리 복구를 다시 확인했다. 일반 사용자 초대·비밀번호
변경부터 native SSH 전달, 실제 API 도구 확인, 계산/자료 읽기, 서버 재시작 후 답변·근거
재조회까지 합성 제공업체로 검증했다. 복합 읽기 도구 수 제한은 이후 별도 단위로 수정했다. 기억/자료 변경은 다음 단위로 수락했으며 개인 Calendar 변경은 후속 단위로 연결했다. 미디어·백그라운드 및 실계정은 남는다.
[검증](reviews/2026-11-14-personal-chatgpt-tools.md).

## 최근 수락한 단위 · 제안에서 공개 스킬 발견

운영자가 설정한 공개 GitHub 목록을 제안 화면에서 읽고, 선택한 패키지의 실제 원본을
확인한 뒤 계정별로 추가한다. 하루 간격/ETag와 오프라인 캐시를 사용하며 설치만으로
작업을 시작하지 않는다. 기본 공개 서버나 게시하지 않은 목록을 연결했다고 표시하지 않는다.

독립 검토에서 새 버전 미리보기가 기존 설치본의 시작 상태를 바꾸는 문제와 첫 진입의
목록 연결 상태 누락을 재현·수정했다. Linux 회귀·수정본 검사, React와 실제 Chrome의
설치/계정 분리/오프라인 복구를 통과했다. 실제 공개 목록 게시·실기기·개인 학습 공유는
별도다. [검증](reviews/2026-11-14-community-catalog-runtime.md).

## 최근 수락한 단위 · 개인 Linux 서버에 ChatGPT 연결

로그인하는 컴퓨터의 native 도우미에서 공식 인증을 마치고, 기존 SSH 연결을 통해 실행 중인
개인 HTTPS 서버로 전달한다. 서버가 별도로 서명·현재 사용자·연결 상태를 확인하며,
전송 전용 로그인은 발급 시부터10분으로 제한된다. 일반 비밀번호 변경·볼트·대화 권한은 없다.

독립 검토의 네 결함을 수정하고 Linux 전체 관련 회귀 검사를 통과했다. 실제 OpenSSH·
Rust API·SQLite·합성 서명 제공업체를 연결해 정상 전달, 응답 유실, 잘못된 서버 키,
검증 중 서버 강제 종료 후 같은 요청 복구와 저장 대화 재조회를 확인했다. 실제 계정·
휴대폰·운영 HTTPS/SSH·서명 installer 검증은 아니다. 이후 개인 foreground 도구는
별도 수락했으며, 개인 Calendar 변경도 후속 수락했다. media/background는 남는다. [검증](reviews/2026-11-14-chatgpt-vm-handoff.md).

## 최근 수락한 단위 · 다른 앱의 글과 링크 공유받기

제목·글·URL을 제한된 임시 공유함으로 받아, 현재 계정에서 편집한 뒤 초안에 한 번만
넣는다. 자동으로 메시지를 보내거나 공유 URL을 방문하지 않는다. 기존 초안을 보존하고
삽입 후 편집창을 접어 입력창으로 돌아온다. 세션 철회·다른 탭의 늦은 응답·재로그인
경합을 독립 재현·수정하고 Native/Linux·통합 React106·실제 Chrome 검사를 통과했다.

Chrome의 실제 multipart navigation·서비스 워커 중지/재시작·사용한 링크 재사용 차단·
다른 탭 로그아웃을 확인했다. 휴대폰 OS의 공유 메뉴나 실제 기기 수락은 별도다.
[검증](reviews/2026-11-14-incoming-pwa-share.md).

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
이 단위는 같은 컴퓨터의 text 범위다. 후속 native Linux 전달은 별도 검증 기록에
연결했고, 개인 foreground 도구도 후속 단위로 수락했다. 개인 Calendar 변경도 후속 수락했다. media·background는 남는다. 실계정 로그인 수락은 아니다. [검증](reviews/2026-11-14-personal-chatgpt.md).

## 최근 수락한 단위 · 저장된 기억 작업 재시도

재시도 중 이미 확정한 기억 작업이 다른 분류 단계에 막혀 실패하던 실행 순서를 수정했다.
같은 저장된 작업을 현재 권한으로 이어서 완료하며 모델에 새 계획을 요청하지 않는다.
기존 실패 테스트를 바꾸지 않고 Linux 에이전트·저장소 검사와 독립 재검토를 통과했다.
[검증 기록](reviews/2026-11-14-sealed-memory-retry.md).

## 최근 수락한 단위 · 공개 스킬 catalog 생성

정확한 공개 GitHub commit과 package 경로에서 검증한 스킬 파일을 가져와 재현 가능한
정적 catalog와 설치용 artifact를 만든다. 개인 자료나 로컬 미커밋 파일을 읽지 않는다.
출력 경로의 다른 사용자 변조 가능성과 macOS의 빈 ACL 처리 결함을 독립 검토로 발견·
수정하고 macOS/Linux 실행 검사를 통과했다. 생성 단위에 이어 설정한 공개 목록의 발견과
선택 설치를 연결했으며 자동 활성화는 하지 않는다. [생성 검증](reviews/2026-11-14-community-catalog.md)·[발견 검증](reviews/2026-11-14-community-catalog-runtime.md).

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
같은 컴퓨터의 로그인 helper·text capability/선택·대화와 보호된 원격 VM 전달을 연결했다. 개인
foreground 도구는 별도 수락했고, 개인 Calendar 변경도 후속 수락했다. media·background 및 실계정 수락은 남는다.

## 계획 단계와 현재 위치

[기존 P0~P10 계획](plan.md#단계별-진행)과 [이전 순서](migration.md#단계와-종료-조건)를
작업 기준으로 사용한다. 아래의 부분 구현을 단계 전체 완료로 계산하지 않는다.

| 계획 단계 | 현재 구현 | 아직 닫지 못한 종료 조건 |
| --- | --- | --- |
| P0 공개 기반 | 공개 저장소·Rust/React/Linux 빌드·기여/OpenSpec | 고정 비교 baseline·독립 평가 runner/grader |
| P1 영속 상태 | SQLite 격리·실행/외부 영수증·journal·봉인된 현재 복원 | ongoing/off-host journal·과거 복원 |
| P2 대화 | 계정·설정·스트림·취소/재시도·개인 ChatGPT | 지원 구독의 전체 실행 범위·실계정 검증 |
| P3 자료·기억 | 출처·기억 정정/망각·제한 추출·문서 | 장기/시간 사실·추출 품질의 독립 검증 |
| P4 도구·학습 | typed 도구·bounded 실행·Git 스킬·제안·읽기 미션 | 성공 절차 학습·독립 두 입력 proof·학습 export·복합 업무 보완 |
| P5 예약·소식 | due loop·반복 회차·알림·quiet hours·Push | 알림 정책/topic mute·실기기 수신·예약 외부 작업 |
| P6 외부 연결 | Gmail/Calendar·공식/커스텀 수집·공개 웹·브라우저 | provider draft·승인된 예약 발송·gws 초대·배송/통화 feed·실계정 |
| P7 첨부·모바일 | 이미지/파일/PDF 텍스트·녹음·글/링크 공유 | 파일/음성 공유·모바일/접근성·실기기 |
| P8 독립 볼트 | 암호문 서비스·클라이언트·CSV/TOTP·현재 복구·실험적 broker 기반 | broker 실제 adapter/사용 흐름·호환 이전·독립 최신 복구 |
| P9 이전·전환 | 계약과 제한된 합성 복구 | private importer·대조·drain/cutover/rollback 실증 |
| P10 출시 | 개발 프리뷰와 공개 문서 | 간편 설치/update/uninstall·서명/SBOM·품질/기기/출시 gate |

수락한 Calendar 단위는 P6으로, broker 기반 단위는 P8으로 추적한다.
두 단위의 수락 범위를 유지하며 남아 있는 P0 평가 기준과 P4 업무·학습 검증부터 의존순서대로
진행한다. 새로운 내부 작업명을 제품 계획의 P 번호로 사용하지 않는다.
CI는 사용자 지시에 따라 계속 비활성이다.

## 다음 구현과 출시까지 남은 일

진행 중인 단위는 위 표의 단계에 연결하고, 다음 미완료 계약을 기존 계획의 의존순서대로
닫는다. `선택` 연결/볼트 패키지도 v1 제공 대상이며 사용자의 설치 선택을 뜻한다.

1. **P0 평가 기준·P1/P2 기초의 남은 계약**: source/config를 고정한 격리 baseline과
   독립 상태/영수증 grader, 실패를 포함한 latency/usage 집계를 먼저 연결한다.
   ongoing/off-host journal·과거 복구, 계정 삭제/outbox·지원 구독 실행 범위·gws 초대는
   해당 저장/연결 의존성과 함께 닫는다.
2. **P3/P4 기억·학습·업무 수행**: 메모리/대화/미션/스킬의 독립 평가,
   typed 성공 절차와 독립 두 입력 proof·합성 export/catalog·예약 needs_skill/GC,
   여러 턴 명확화·넓은 시간 해석·계획/검증/보완을 구현한다.
3. **P5/P6 예약·외부 연결**: followup/topic mute/알림 정책,
   Gmail provider draft·승인된 고정 예약 발송, Calendar 후속 명확화,
   통화 feed·opt-in 작업 세션·공식 배송 parser와 실제 OAuth/계정 검증을 진행한다.
   Calendar 미션·반복/종일/참석자/보조 캘린더는 현재 지원 범위에 포함하지 않는다.
4. **P7 첨부·모바일**: 파일/음성 공유, 실제 음성 인식·OS 공유 메뉴·Push·키보드/스크롤·
   접근성을 지원 기기에서 검증한다.
5. **P8 볼트와 credential 사용**: 수락한 broker 기반에 실제 typed adapter·최초 연결/
   잠금 대기/자동 재사용 UX를 연결하고, online commitment/revoke·offline resume·
   secret 비노출·독립 최신 복구·구 v1 호환/이전·계정 삭제 outbox를 검증한다.
6. **P9/P10 이전·출시**: private importer/대조·제한 cutover·rollback,
   signed Linux x86_64/arm64 설치·update/drain/uninstall·SBOM,
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
