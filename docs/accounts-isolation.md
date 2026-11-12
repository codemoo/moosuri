# 사용자·환경·세션 격리

한 설치에 여러 계정을 둔다. 설치할 때 admin 하나만 만들며 추가 계정은 관리자가 이름·이메일을 입력해 초대한다. 공유하는 것은 모델 연결·추론 용량·공개 절차 catalog다. 대화·메모리·자료·학습 절차·외부 계정·실행 환경은 개인별이다. 이 문서는 구현 계약이며 현재 기능 완료가 아니다.

## 식별자와 저장 경계

`AccountId`는 바뀔 수 있는 이름/이메일과 분리한다. v1은 계정 하나에 개인 `TenantId` 하나이며 그룹 공동 문서는 후속 설계다. account와 tenant가 같은 값이더라도 다른 타입으로 둔다. 설치 관리 `AdminContext`, 인증된 `UserContext`, durable 작업의 `ExecutionContext`를 분리한다. browser/model/header가 다른 user ID를 넣어 context를 만들 수 없다.

assistant.sqlite 하나에 설정·계정·사용량·개인 테이블을 둔다. 모든 개인 객체의 조회/수정은 `tenant_id`를 필수로 받는 repository를 통한다. `(tenant_id,id)` unique와 composite FK를 사용한다. ID만으로 전역 조회하는 메서드는 두지 않는다. SQLite connection은 store 내부에만 노출한다. 계정/예산/작업 접수를 같은 DB transaction으로 묶고 vault와는 outbox/journal로 동기화한다.

| 영역 | 반드시 사용자에 귀속되는 것 |
| --- | --- |
| 인증 | session digest·기기·auth epoch·password version·로그인 단계·Push binding |
| 대화 | thread/message/request/run/attempt·첨부·read set·checkpoint·event cursor |
| 기억·자료 | source binding/cursor/scope·FTS index·인용·memory/revision·망각 tombstone |
| 작업 | task/plan/acceptance·schedule/occurrence·effect/receipt·budget·notification |
| 학습 | 절차·slot·성공 proof·실제 사용·실패 집계·discovery cache |
| provider | 개인 Google/Claude/ChatGPT 연결·remote conversation/file ID·credential refresh state |
| 파일 | user home/workdir/temp/upload/cache/export·child process·artifact reference |

FTS는 사용자 scope 밖의 후보/제목/snippet을 반환하지 않는다. 모든 cache key는 tenant와 필요한 revision을 포함한다. SSE는 사용자별 cursor이며 다른 사람의 event sequence나 payload를 읽을 수 없다. 객체를 찾지 못한 것과 다른 사용자 소유인 것을 구별해 정보를 흘리지 않는다.

관리자 API는 계정/역할/연결/할당량/비용 집계만 제공한다. admin 역할이 일반 repository의 scope를 해제하지 않는다. 사용자 가장하기와 private 내용 조회 화면은 만들지 않는다. 단, 일반 대화/connector token은 서버가 처리하므로 OS/DB 관리자를 암호학적으로 차단하는 E2EE 보장은 아니다. **별도 vault만 사용자 키로 그 위협을 방어한다.**

## 로그인과 계정 수명

최초 admin은 입력한 비밀번호 또는 생성된 임시 비밀번호를 쓴다. 추가 사용자는 기본24시간 유효한 임시 credential로 로그인 후 강제 변경한다. 재발급은 이전 credential을 폐기한다. DB에는 Argon2id password hash·session/invite token digest만 저장한다. 이메일 uniqueness는 문서화한 normalization으로 처리하며 Gmail의 dot/plus 규칙을 모든 주소에 적용하지 않는다.

`password_change` 제한 session은 비밀번호 변경/로그아웃/최소 계정 상태만 접근한다. chat/SSE/upload/모델/Push/vault/admin 접근은 서버에서 거절한다. 새 비밀번호 저장·temp 소비·옛 session 폐기·정식 session 발급은 원자적으로 처리한다. 두 번 교환하면 한 요청만 성공해야 한다.

정식 session은 기본30일과 활동 갱신, 기기별 로그아웃, 전체 기기 폐기를 제공한다. Secure/HttpOnly/SameSite cookie, CSRF/Origin 확인, login 회전·rate limit을 적용한다. 실제401과 잠깐의 네트워크 실패를 구분한다. 로그인 유지가 vault unlock 유지를 뜻하지 않는다.

password reset/disable/delete/중요 role 변경은 auth epoch를 올리고 이전 session·stream·Push·새 작업을 막는다. 이미 dispatch한 외부 effect는 좁은 reconciliation 경로에서 영수증만 수집하고 재발송하지 않는다. 마지막 active admin을 무심코 없애는 변경은 transaction 제약으로 거절한다. 로컬 operator 복구는 기록 가능한 별도 CLI이며 볼트 해독 우회로가 아니다. 임시 비밀번호를 가진 admin/operator는 일반 앱 계정 복구 경로를 통제할 수 있으므로 일반 대화에 대해 악성 operator 방어를 주장하지 않는다.

## 유지되는 세션과 작업 환경

서버 재시작/브라우저 재접속은 본인의 마지막 대화, 미완료 run checkpoint, 예약, 설정을 복원한다. 여러 기기의 last-thread/Push는 기기와 account에 묶는다. 모델 연결과 app login, provider OAuth, vault unlock은 서로 다른 수명이다.

provider remote ID는 `(tenant, provider_instance, provider_version, local_thread)` mapping을 통해서만 재개한다. `resume latest`를 전역으로 호출하지 않는다. SIWC처럼 원격 conversation 저장을 지원하지 않는 연결은 local durable history를 보낸다. 공유 API key/HTTP pool/local inference process가 공용 대화 버퍼나 mutable agent session을 뜻하지 않는다.

파일 경로는 불투명 내부 ID로 만든 `users/<tenant>/...` 아래로 제한한다. email·model output·사용자 path 문자열로 경로를 만들지 않는다. canonical path/symlink 경계를 검사하며 user별 private temp와 저장 한도를 둔다. 기본 agent는 임의 shell/file access를 갖지 않는다.

gws/공식 agent CLI는 binding별 fresh process·명시적 env·private home/config/cwd·부모 종료 정리를 사용한다. `.env` 자동 로드와 OS keyring을 포함해 다른 계정의 credential을 상속하지 않는다. same UID의 폴더 분리만으로 임의 코드를 격리했다고 말하지 않는다. 파일/코드 실행 가능한 CLI는 별도 OS UID 또는 검증한 Linux sandbox/container, mount/process/network/resource 제한이 통과해야 활성화한다. 통과 전에는 해당 연결을 unavailable로 둔다.

## 공유 브라우저와 늦은 응답

React의 모든 비동기 작업은 account/session generation을 캡처한다. A 로그아웃→B 로그인 뒤 A의 HTTP/SSE/upload/녹음/Worker 결과를 버린다. A의 늦은 logout 응답이 B의 새 초안을 지우면 안 된다. query cache·draft·object URL·미디어 track·share draft·stream·Push binding을 수명에 맞게 정리한다. service worker에는 공개 shell만 저장한다. 로그인 전 share 입력은 제한된 일회 ephemeral draft로 두고 계정 전환 후 자동 전송하지 않는다.

Push payload는 private 본문 없이 opaque ID만 갖는다. endpoint의 소유권 세대가 달라지면 오래된 event를 열지 않고 현재 계정의 로그인 후 조회를 요구한다. user A의 vault Worker도 account switch/logout 때 종료하며 B의 세션으로 복호화 결과가 넘어가지 않는다.

## 공용 용량·사용량

사용자별 round-robin/가중 fair queue, 사용자 실행 상한, 전체 provider 상한을 둔다. interactive chat과 background는 명시된 우선순위로 처리하며 starvation을 시험한다. 한 transaction에서 user·task·공용 budget을 모두 reserve한 후 모델을 호출한다. `invocation_id/reservation_id`로 정확히 한 번 정산한다. 취소/timeout/연결 끊김은 무료 증거가 아니므로 unknown 사용량을 무조건 환불하지 않는다.

원장에는 account/tenant/run/step/provider version/model/period/timezone·input/output/cache/reasoning token·실제/추정/불명·통화/가격표 version을 기록한다. nested 검색·추출·media·검증 호출도 원사용자에 귀속한다. 설치 유지 작업은 system bucket이다. 사용량을 모르면 null/unknown이며0으로 표시하지 않는다. 구독 잔여량을 토큰 추정으로 만들어내지 않는다. local 모델 비용은 API 청구액과 구분한다.

관리자는 사용자/기간/model별 집계와 queue/error/latency를 보고, 사용자는 자기 내역만 본다. 프롬프트·답변·문서 제목·메일 수신인·볼트 항목 수는 사용량 화면에 넣지 않는다. 삭제된 계정의 최소 회계 기록은 opaque ID로 retention 정책에 따라 남긴다.

## 복원과 필수 회귀

계정 disable/delete/auth epoch와 개인 삭제는 독립 journal에 남긴다. 옛 DB 복원으로 초대·reset token·삭제 사용자·예약·권한을 부활시키지 않는다. 새 가입에 옛 tenant를 재사용하지 않는다. 복원은 전체 session/invite 폐기·미완료 작업 격리·최신 journal 반영에 더해 모든 옛 password hash의 새 로그인 사용을 차단한다. private 로컬 recovery CLI로 새 admin credential/현재 역할을 기록하고, 나머지 사용자는 새 임시 credential과 강제 변경으로 재등록한다. 과거 admin role/provider policy는 자동 활성화하지 않는다. provider bindings는 현재 정책/credential 재등록까지 disabled다. 이는 정상 재시작과 다른 restore-only 절차이며 vault master/recovery key는 별도다.

필수 시험은 A/B 동일 검색어·겹치는 ID로 모든 content API/FTS/pagination/SSE/tool/파일/receipt 접근 거절, composite FK crossing 거절, temp-session 우회 거절, concurrent budget/초대 소비, provider session 혼선0, 계정 전환 중 held response, 실제 SQLite kill/restore, CLI mount/env/symlink 탈출 거절이다. 단순히 화면을 숨기는 테스트로 격리를 증명하지 않는다. [features](features.md)의 F56~F64에 release gate로 연결한다.


복원 시 snapshot 이후 사용량·reservation을 모두 알 수 없으면 영향받은 사용자/공용 기간을 accounting_unknown으로 표시하고 해당 예산의 추가 지출을 막는다. 남아 있는 정산·reservation은 하한으로 유지하며0으로 초기화하지 않는다. provider/독립 accounting evidence로 reconcile하거나 관리자의 명시적 보수 조정·새 예산 배정 후 앞으로의 호출만 허용한다. unknown 과거 이력은 계속 표시한다. 별도 usage ledger의 독립 내구성을 실제 구현·검증하기 전에는 무손실 회계 복구를 약속하지 않는다. snapshot 뒤 비밀번호 변경·role 하향·provider 폐기·charge·pending reservation 후 restore하는 회귀가 필수다.

## 스킬·credential의 사용자 범위

community package bytes는 공개 immutable cache로 공유할 수 있지만 설치 여부·선택 버전·개인 수정·dismiss/mute·학습 proof·일정 의존성은 tenant별이다. 배포자나 다른 사용자의 성공 기록을 자신의 학습 proof로 승계하지 않는다. credential binding·device pairing·grant·대기 run/effect도 tenant에 귀속하며 admin role로 다른 사용자의 vault handle을 해석할 수 없다. [community](community-skills.md), [credential-use](credential-use.md)의 lifecycle을 SEC01/CRED03/COMM02에서 함께 검사한다.

remote broker도 계정 epoch/철회와 [dispatch commitment](credential-use.md)의 순서를 따른다. 계정 정지는 아직 committed되지 않은 새 작업을 막지만 이미 committed된 외부 작업은 in-flight/unknown으로 남을 수 있다. 기기 ACK가 없으면 local 철회까지 완료했다고 표시하지 않는다. 이 경계는 볼트 평문 비노출과 별도로 검증한다.
