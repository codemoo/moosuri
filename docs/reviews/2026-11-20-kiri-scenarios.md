# Kiri 회원 기능 시나리오

Chrome·WebKit 시나리오는 정적 Kiri 웹 빌드에 로그인, 목록, 저장 및 재시도 응답을 합성해서 검증한다. 실제 버튼·링크·입력창과 브라우저 방문 기록을 사용한다. Google 동의와 볼트 새 탭은 브라우저 수준에서 가로채며 루프백 외 네트워크 요청을 차단한다. 실제 게이트웨이와 회원 DB를 거치는 여정, Claude 대화의 문항과 실행 상태도 아래에 기록한다. 실제 Google 동의·조회, 모바일 실기기와 운영 서버 전환은 남아 있다. [장기 대화 측정](../../audit/memory-reader/results.md)

## 실행

Python 3.10 이상, Playwright Python, 설치된 Google Chrome이 필요하다. WebKit은 Playwright 버전에 맞는 WebKit 런타임을 사용한다. 저장소 루트에서 실행한다. `--scenario B-CHAT-02` 또는 `--width 390`을 반복하여 실행 대상을 선택할 수 있다. `--headed`는 눈으로 화면을 보며 재현할 때 쓴다.

```sh
npm ci --ignore-scripts --prefix web
npm run build --prefix web
python3 deployment/kiri/linux-vm-smoke/member-ui-scenarios.py web/dist
python3 deployment/kiri/linux-vm-smoke/member-ui-scenarios.py web/dist --browser webkit
npm test --prefix web
cargo test --locked -p moosuri personal_gateway
cargo test --locked -p moosuri-store personal_tasks
```

아래 표의 `B-`는 브라우저 시나리오 ID다. `V`는 표시한 웹 테스트 파일의 테스트 이름으로 찾고, `R`은 Rust 테스트 함수 이름으로 찾는다. 결과 집계에서는 같은 `B-`의 360·390·1440px 실행을 한 시나리오로 센다. 원래 대화 열기처럼 여러 단계를 밟는 경우에도 표의 한 행을 한 시나리오로 센다.

## 실행 결과

Chrome 158과 WebKit 26에서 서로 다른 브라우저 조작 38개를 각각 360·390·1440px로 확인했다. Chrome 114건, WebKit 114건이 통과했다. WebKit 첫 실행에서는 360px Drive 연결 해제 시나리오가 문서 로딩 단계에서 시간 초과했다. 새 브라우저에서 문서 준비와 실제 조작의 시간 제한을 분리해 남은 폭과 서비스를 다시 실행했다. 이 수치에 iPhone·Android 실기기는 포함되지 않는다.

별도 로컬 TLS 게이트웨이와 두 회원 DB의 여정 6건 및 표준 서버 HTTP·런타임·SQLite의 `moosuri-eval` 15건이 통과했다. 여기서 모델은 시험 응답을 반환했다. 실제 Claude Sonnet 5.5 medium은 별도로 산술, 시간표 판독, 주어진 정보로 답할 수 없는 질문 세 개에서 기대 결과를 반환했다. [모델별 검증 범위](../../audit/memory-reader/results.md)

## 로그인과 세션

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 초대 비밀번호로 로그인하고 변경한 뒤 새로고침하고 로그아웃한다 | 변경 전 개인 API 호출이 없고 변경 후 대화가 열린다. 로그아웃 후 새로고침해도 로그인 화면이다. | `B-AUTH-01` |
| 전송 결과를 받기 전에 로그아웃하고 다른 회원으로 로그인한다 | 이전 응답과 작성 중인 문장이 새 회원 화면에 나타나지 않는다. | V `PersonalPreview.test.tsx` · `logout fences an in-flight response` |
| 두 회원으로 로그인하고 런타임 재시작과 비밀번호 변경을 거친다 | 원래 회원의 세션과 볼륨만 쓰고 이전 세션은 철회한다. | R `two_private_runtimes_login_session_restart_password_and_logout_preserve_owner` |
| 두 회원 소켓에서 같은 게이트웨이로 자기 대화를 읽는다 | 쿠키와 대화 결과는 로그인한 회원 경계 안에 있다. | R `actual_member_sockets_keep_cookie_locator_and_thread_history_private` |

## 대화 작성과 이어가기

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 작성 예시를 넣은 뒤 문장을 키보드로 바꾸고 기억을 거쳐 다시 돌아와 Enter로 보낸다 | 예시는 초안으로만 들어가며 제출 시 대화와 요청이 하나씩 생긴다. 새로고침과 날짜별 목록 검색으로 같은 응답을 찾는다. | `B-CHAT-01` |
| A를 전송하고 기다리는 동안 B를 입력한 다음 A를 거절한다 | 새로고침해도 A와 B가 나뉜다. 명시적으로 A만 같은 요청 번호로 재전송하며 B는 남는다. | `B-CHAT-02` |
| 요청은 처리했지만 HTTP 응답을 잃은 뒤 새로고침한다 | 자동 POST 없이 저장 결과를 찾고 명시 재시도에서 같은 요청 번호를 쓴다. | V `PersonalPreview.test.tsx` · `recovers an unconfirmed send after refresh` |
| 검색 결과의 다른 대화를 선택하지 않고 삭제한다 | 현재 대화와 초안은 유지되며 확인 후 대상만 삭제된다. | V `PersonalPreview.test.tsx` · `confirms deletion of a searched conversation` |
| 대화 목록 조회가 실패한 뒤 다시 시도한다 | 실패와 빈 목록을 구분하고 다시 열린 목록에서 대화를 찾는다. | V `PersonalPreview.test.tsx` · `makes an unavailable inventory distinguishable from empty history` |
| 한글 조합 중 Enter를 누른다 | 조합 완료 단계에서는 메시지를 전송하지 않는다. | V `PersonalPreview.test.tsx` · `does not submit an Enter that completes Korean composition` |
| 모델 확인이 지연되는 동안 저장 대화를 연다 | 기존 기록이 바로 보이고 모델 상태 오류 뒤에도 남는다. | V `PersonalPreview.test.tsx` · `opens a saved conversation while model discovery stalls` |
| 개인 서버로 텍스트 요청을 전달하고 저장된 이력을 읽는다 | 게이트웨이·회원 소켓을 지나 한 번만 요청하며 새 조회에서 같은 기록을 반환한다. | R `opt_in_preview_crosses_gateway_and_member_socket_once_and_uses_saved_history` |

## 기억

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 작성 중 Escape 및 화면 이동 후 재개하고, 저장한 기억을 다시 수정한다 | 초안을 보존하고 두 저장 버전의 내용과 시각을 표시한다. | `B-MEM-01` |
| 화자 검색으로 열린 항목을 편집한다 | 최신 값을 표시하고 편집·상세를 닫으면 목록의 같은 항목으로 초점이 돌아간다. | V `PersonalMemories.test.tsx` · `searches by speaker` |
| 검색 결과가 다음 묶음에만 있다 | 유한한 묶음씩 이어서 찾고 검색 지우기로 전체 목록을 복원한다. | V `PersonalMemories.test.tsx` · `searches other pages in bounded batches` |
| 구버전을 대상으로 삭제 확인 후 다시 시도한다 | 최신 기억을 읽고 그 버전 삭제에 다시 동의를 받는다. | V `PersonalMemories.test.tsx` · `reloads after a stale deletion` |
| 기억 쓰기는 저장됐지만 응답이 유실된다 | 화면 이동 뒤 같은 요청 번호로만 복구한다. | V `PersonalMemories.test.tsx` · `keeps an uncertain save request across views` |

## 화면 탐색과 커넥터 목록

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 기억 → 연결 → Gmail을 브라우저 뒤로·앞으로 이동하고 상세를 새로고침한다 | 탐색 메뉴, 목록, Gmail 상세를 순서대로 복원한다. | `B-NAV-01` |
| 맡긴 일 상세에서 Gmail 동의를 취소하고 브라우저의 뒤로·앞으로 버튼을 누른다 | 상세 → 커넥터 목록 → 원래 업무로 돌아가고 앞으로 이동하면 다시 Gmail 상세가 열린다. 권한 교환과 조회는 없다. | `B-NAV-02` |
| 지원되지 않는 서비스와 한국어 별칭을 검색한다 | 빈 검색 결과를 구분하고 Google Docs 상세에서 같은 검색으로 돌아온다. | V `PersonalPreview.test.tsx` · `searches supported services and returns to the filtered list` |
| 설치 관리가 준비 중이고 개별 회원이 미연결 상태다 | 설치 상태와 회원 상태를 구분하며 상세를 닫으면 원래 커넥터 행에 초점이 돌아온다. | V `PersonalGoogleWorkspace.test.tsx` · `keeps an unregistered installation distinct` |
| 회원이 관리자 주소를 직접 연다 | 대화로 열리고 관리 메뉴와 관리자 요청이 없다. | `B-ADM-03` |

## Google 서비스별 연결

각 행의 성공·취소·연결 해제는 새 브라우저 컨텍스트에서 독립적으로 실행한다. 성공은 기억에서 이동해 동의하고 계정 표시와 상세 새로고침을 검사한다. 취소는 관리자 엔진에서 이동해 거절하며 권한 교환이 없고 뒤로 두 번 가면 엔진에 돌아간다. 해제는 연결된 합성 계정을 보면서 현재 연결 ID와 버전으로 요청하고 미연결 상태를 확인한다. 목록과 단순 연결에서는 Gmail 받은편지함을 읽지 않는다.

| 서비스 | 성공 | 취소 | 해제 |
| --- | --- | --- | --- |
| Gmail | `B-GMAIL-01` | `B-GMAIL-02` | `B-GMAIL-03` |
| Google Drive | `B-DRIVE-01` | `B-DRIVE-02` | `B-DRIVE-03` |
| Google Docs | `B-DOCS-01` | `B-DOCS-02` | `B-DOCS-03` |
| Google Slides | `B-SLIDES-01` | `B-SLIDES-02` | `B-SLIDES-03` |
| Google Sheets | `B-SHEETS-01` | `B-SHEETS-02` | `B-SHEETS-03` |

| 추가 Gmail 읽기 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 이미 연결한 Gmail 상세를 보고 대화에서 받은편지함 읽기를 명시적으로 요청한다 | 연결 상세만 볼 때는 읽지 않으며 요청 후 원래 대화에서 메일 제목을 표시한다. 새로고침은 저장된 기록만 읽는다. | `B-GMAIL-04` |
| 대화에서 명시적으로 받은편지함 읽기를 요청하고 동의한다 | 서버가 이어갈 대화를 지정하며 원래 대화에 읽은 결과를 저장한다. | V `PersonalGoogle.test.tsx` · `returns the durable read to the original thread` |
| 읽기 동의를 취소하거나 새 세션에서 이전 핸들을 연다 | 메일 열람과 권한 교환을 시작하지 않는다. | V `PersonalGoogle.test.tsx` · `denial clears pending read` 및 `a different session cannot exchange the returned handle` |
| 게이트웨이에서 다른 회원의 콜백을 처리한 뒤 원래 회원이 연다 | 콜백을 시작한 회원만 교환하며 저장된 읽기를 재개한다. | R `google_callback_claims_only_the_originating_member_and_resumes_a_pinned_read` |

## 개인 볼트 연결

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 등록된 볼트 링크를 누른다 | 별도 로그인 안내와 함께 새 탭이 열리고 회원 연결 상세는 유지된다. | `B-VAULT-01` |
| 볼트 상태 조회가 실패하고 다시 확인한다 | 네트워크 실패와 주소 미등록을 구분한다. | V `PersonalPreview.test.tsx` · `distinguishes unavailable vault status` |
| 등록 설정에 현재 앱과 같은 주소 또는 안전하지 않은 주소가 들어 있다 | 사용자에게 새 탭 링크를 노출하지 않는다. | V `PersonalPreview.test.tsx` · `does not link to an unsafe vault origin` |

## 회원 관리

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 초대가 접수된 뒤 응답이 끊기고 관리 메뉴를 나갔다가 새로고침한다 | 동일한 요청 번호와 이메일로 한 번만 등록하며 접수증을 다시 열 수 있다. | `B-ADM-01` |
| 활성 회원을 표시 이름과 이메일로 찾고 정지 확인을 취소한 다음 다시 승인한다 | 취소 시 작업이 없고 버튼에 초점이 돌아온다. 접수 응답 유실 후 같은 요청으로 정지 결과를 확인한다. | `B-ADM-02` |
| 마지막 관리자를 정지하거나 권한을 변경하려고 한다 | 마지막 관리자 보호가 적용되며 또 다른 준비된 관리자가 생기면 다시 판단한다. | V `PersonalAdmin.test.tsx` · `protects the last available administrator` |
| 관리자 권한이 철회된 기존 로그인에서 관리 API를 요청한다 | 호스트 역할 검사 후 변경이 거부된다. | R `private_member_session_and_host_role_control_web_admin_even_after_demotion` |

## Google 설치 설정

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 관리자 화면에서 기존 Google 웹 클라이언트 JSON을 가져온다 | 잘못된 콜백을 수정하고 다시 제출하면 등록 성공을 보여 준 뒤 회원 관리로 돌아간다. | `B-GWS-01` |
| 회원 관리에서 Google 설정을 열고 `커넥터 열기`를 선택한다 | 커넥터 목록으로 이동한다. | `B-GWS-02` |
| Gmail 상세에서 설정을 열고 새로고침한 뒤 화면의 뒤로 버튼을 누른다 | 설정에서 Gmail 상세로 돌아간다. | `B-GWS-03` |
| Google 프로젝트와 다섯 API 준비 중 화면을 다시 연다 | 프로젝트 ID와 진행 단계가 남고 공식 콘솔과 OAuth 등록 경로를 표시한다. | V `PersonalGoogleSetup.test.tsx` · `resumes the selected project` |
| Google 설정 등록 응답이 충돌하고 다른 관리자가 먼저 저장한다 | 저장된 설정을 다시 확인해 실제 설치 상태를 표시한다. | V `PersonalGoogleSetup.test.tsx` · `uses the stored status to resolve a conflicting upload` |
| 파일을 읽는 동안 현재 계정이 바뀐다 | 이전 계정이 선택한 JSON을 전송하지 않는다. | V `PersonalGoogleSetup.test.tsx` · `does not transmit a pending file read` |

## 엔진·공급자 관리

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 정지된 엔진에서 새 추론 강도를 선택하는 동안 다른 관리자가 변경한다 | 충돌과 현재 값을 표시하고 새 버전으로만 저장한다. 엔진을 다시 허용하면 기억을 거쳐 돌아온 대화에서 입력과 전송이 된다. | `B-ENG-01` |
| 엔진이 구성되기 전후에 관리 화면을 연다 | 서버 등록 안내와 공급자 기본값이 상태에 맞게 표시된다. | V `PersonalEngineSettings.test.tsx` · `offers server setup guidance` |
| Claude Code·ChatGPT 구독·OpenAI API 등의 다른 연결에서 사용량을 확인한다 | 등록 방식별 공식 확인 경로를 보여 주고 미제공 한도와 회원 집계를 구분한다. | V `PersonalEngineSettings.test.tsx` · `directs %s / %s admins to the matching usage source` |
| 기존 일반 회원 탭이 엔진 재개 후 다시 초점을 얻는다 | 새 대화를 열 수 있고 늦은 과거 상태 응답이 다시 잠그지 않는다. | V `PersonalPreview.test.tsx` · `rechecks a paused engine when an existing member tab` 및 `ignores an older capability response` |

## 브라우저 작업

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 대화에서 사이트를 열고 추가 지시를 보낸 다음 미리 보기·직접 조작·탭 변경·스크롤·작업 넘기기를 실행한다 | 현재 프레임과 탭 정보가 쓰이고 맡긴 요청과 추가 지시 결과가 원대화에 보인다. 뷰어만 닫으면 작업 중지 요청은 없다. | `B-BR-01` |
| 프레임 유효 기간 후 작업을 넘기거나 브라우저가 바쁠 때 다시 시도한다 | 최신 화면을 확인하고 오래된 입력은 막으며 제어권을 유지한다. | V `PersonalPreview.test.tsx` · `renews an idle frame proof before Done` |
| 일반 채팅에 참고용 URL을 넣거나 브라우저 기능이 준비되지 않은 상태에서 페이지 열기를 요청한다 | 일반 URL은 채팅으로 보내고 기능이 없을 때 작업 문장을 초안에 보존한다. | V `PersonalPreview.test.tsx` · `keeps an ordinary reference in chat` |

## 맡긴 일

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 오래된 업무 상세를 열고 새로고침 뒤 원래 대화로 간다 | 저장된 요청·결과·이력을 읽고 필요한 대화 페이지를 불러와 원래 요청에 초점을 맞춘다. 새 실행이 생기지 않는다. | `B-TASK-01` |
| 페이지 이동 요청이 실패한 뒤 더 오래된 일을 다시 읽는다 | 이미 읽은 행을 보존하고 같은 커서로 재시도한다. | V `PersonalTasks.test.tsx` · `keeps older rows during pagination failure` |
| 다른 회원에게 속한 상세를 열거나 계정 변경 후 오래된 응답이 도착한다 | 해당 상세가 표시되지 않으며 비어 있는 자기 목록과 구분한다. | V `PersonalTasks.test.tsx` · `drops the old account's in-flight detail` |
| 실제 SQLite를 닫고 다시 열어 완료 업무와 실행 중이던 업무를 확인한다 | 완료 결과를 재조회하며 중단된 업무는 결과 불명과 이력을 표시한다. | R `personal_tasks_reopen_completed_result_and_recover_interrupted_job_with_real_history` |
| 스키마 89에서 생성된 작업을 이전한다 | 기존 결과를 유지하고 기록되지 않은 사건 시각은 미기록으로 남긴다. | R `personal_tasks_schema_89_preserves_existing_result_and_marks_unknown_event_time` |

## 실행 중인 일반 대화의 후속 지시

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 응답 전 같은 탭에서 후속 지시를 저장한다 | 최초 모델 요청의 내용을 유지하며 저장한 지시는 원래 실행에 연결된다. | V `PersonalPreview.test.tsx` · `accepts a follow-up while this tab still awaits its first broker reply` |
| 후속 지시 접수 응답을 잃은 뒤 새로고침한다 | 기존 요청 번호로 접수를 확인하며 지시를 원래 답변에 반영했다고 표시하지 않는다. | `B-FTU-01` · V `PersonalPreview.test.tsx` · `stores a distinct text follow-up and reconciles its original instruction` |
| 원래 응답 완료 후 후속 메시지로 보내기를 누른다 | 진행 중에는 전송이 잠기고, 완료 후 명시적으로 누르면 새 대화 턴으로 한 번만 전달된다. | 같은 V 테스트의 원대화 이력·POST 확인 및 [회원 저장소](../../crates/moosuri-store/src/personal_text_preview.rs) |
| 원래 실행이 먼저 끝나 후속 저장이 충돌한 뒤 새로고침한다 | 작성 중인 다른 초안을 보존하고 회원이 원할 때에만 원문을 입력창으로 복구한다. | `B-FTU-02` |

## 관리자 웹 초대와 호스트 자동 준비

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 실제 관리자로 두 회원을 초대하고 한 회원분의 저장공간만 허용한다 | 타이머가 한 명을 비밀번호 입력 대기로 준비하며 다른 초대는 용량을 기다린다. | Ubuntu 24.04.5·Kiri 0.2.1 [실행 기록](../../deployment/kiri/linux-vm-smoke/onboard-worker-evidence.json) |
| 같은 예약을 재처리하고 운영자가 최초 비밀번호를 입력한다 | 회원·UID·볼륨을 보존한다. 최초 로그인은 비밀번호 변경을 요구하고 기존 자격으로 다른 회원에게 로그인할 수 없다. | 같은 Linux 실행 기록 |
| 저장공간을 두 회원분으로 늘린다 | 남은 회원을 다른 UID에 준비한다. 이미 준비된 회원은 재처리 후 본래 자격으로 로그인한다. | 같은 Linux 실행 기록 |
| 처리 중 이미 연결한 설치 경로를 바꿔 재시도한다 | 준비된 두 회원의 연결 정보를 유지하고 경로 변경으로 인한 재준비를 거절한다. | 같은 Linux 실행 기록 |

## 관리자 OpenRouter 사용 한도

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 관리자가 공식 키 한도 조회를 누르고 0, 미제공 값, 초기화 시각이 있는 응답을 받는다 | 서버 수치와 조회 시각을 표시한다. 값이 없는 항목은 확인 불가로 보인다. | `B-ENG-02` · V `PersonalEngineSettings.test.tsx` · `requests current OpenRouter capacity explicitly` |
| 관리자가 두 탭에서 차례로 조회하고 첫 탭의 결과를 나중에 확인한다 | 진행 중인 요청은 함께 쓰고, 완료한 두 결과는 각각 자기 조회 ID로 유효 기간 안에 읽는다. | R `openrouter_quota_reads_only_on_admin_request_and_fences_key_and_connection_changes` |
| 응답 전 키나 연결을 교체하거나 관리자 권한을 철회한다 | 새 연결과 일반 회원에게 이전 키 사용량을 전달하지 않는다. 만료된 요청은 새 조회가 필요하다. | `B-ENG-03` · R `quota_requests_expire_and_demoted_admin_cannot_trigger_or_receive_shared_key_capacity` 및 위 브로커 테스트 |

## 비회원 예시 대화

| 시나리오 | 기대 결과 | 근거 |
| --- | --- | --- |
| 로그인 전 시간표 예시를 연다 | 합성 시간표와 근거를 표시하고 모델·회원 API는 호출하지 않는다. | `B-DEMO-01` · V `PublicDemo.test.tsx` · `opens the public timetable example` |
| 기억 정정, 맡긴 일을 번갈아 열고 연습 문장을 입력한 뒤 떠난다 | 선택한 예시만 보이며 입력은 브라우저에 저장되지 않고 다시 열면 비어 있다. | `B-DEMO-02` · V `PublicDemo.test.tsx` · `shows only the selected synthetic conversation`, `keeps a practice question out of persistent storage` |
| 회원 공간 열기를 눌러 실제 회원 진입점으로 이동한다 | 회원 로그인 화면에서 세션 조회가 시작된다. 데모의 내용은 회원 계정에 들어가지 않는다. | [Chrome 390·1200px 실측](../../audit/standard-journey-checks.md#f18-합성-미리보기) |

## 실제 게이트웨이를 거치는 회원 여정

[실행기](../../deployment/kiri/linux-vm-smoke/member-web-fullstack.py)는 빌드된 `moosuri` 실행 파일 하나를 인자로 받는다. 브라우저 → TLS 게이트웨이 → 회원별 Unix 소켓과 SQLite → 모델 브로커를 실제 프로세스로 구동한다. 모델 HTTP 응답만 로컬 합성 SSE로 제공한다.

| ID | 회원 조작과 확인한 결과 |
| --- | --- |
| MWF-01 | 첫 로그인과 필수 비밀번호 변경 후 같은 브라우저에서 채팅을 연다. |
| MWF-02 | 두 차례 대화해 실제 브로커가 앞선 질문·답을 전달하는지, 두 응답이 DB에 저장되는지 확인한다. |
| MWF-03 | 검색으로 원래 대화를 다시 열고 입력 중인 초안과 저장 이력을 새로고침 뒤 확인한다. |
| MWF-04 | 기억을 작성·수정한 다음 다시 열어 두 번째 리비전을 읽는다. |
| MWF-05 | 같은 브라우저에서 두 번째 회원으로 로그인해 첫 회원의 대화·초안·기억이 보이지 않는 것을 확인한다. |
| MWF-06 | 첫 회원의 변경한 비밀번호로 복귀하고 저장한 기억과 이력을 삭제한다. |

## Claude 대화 검증

실제 대화 채점에는 공식 Claude CLI의 `claude-sonnet-5-5`와 추론 강도 `medium`을 사용한다. Chrome에서 합성 회원으로 로그인해 두 대화를 연다. 시험용 HTTP 어댑터는 브로커가 보낸 제품 지침과 해당 대화의 질문·답을 확인한 뒤 공식 CLI에 전달한다. 각 응답에서 CLI가 보고한 모델을 확인하고 해당 턴의 화면 본문과 회원 SQLite의 저장값을 대조한다. 운영 회원 소켓의 구독 워커는 별도 운영 기록에서 확인할 수 있다. [운영 확인 기록](2026-11-19-status-history.md)

| ID | 조작 | 기대 결과 |
| --- | --- | --- |
| ML-01 | 23과 19를 더해 숫자만 답하라고 요청한다. | `42` |
| ML-02 | 열차 출발 시각을 14:37로 알려 준 뒤 시각만 묻는다. | `14:37` |
| ML-03 | 같은 대화에서 방금 열차의 출발 시각을 되묻는다. | `14:37` |
| ML-04 | 빈 본문으로 출력 한도가 끝나는 합성 응답을 보낸 뒤 새로고침한다. | ‘답변 미완료 · 표시할 본문 없음’이 유지되고 공급자 요청과 저장된 실행은 각각 한 건이다. |
| ML-05 | 새 대화에서 프랑스의 수도를 한글 지명으로 묻는다. | `파리` |
| ML-06 | 알려 준 적 없는 개인 좌석 번호를 묻고 정보가 없을 때 쓸 답을 지정한다. | `확인 불가` |
| ML-07 | 입실 시각 09:07을 알려 주고 HH:MM 형식을 요청한다. | `09:07` |
| ML-08 | 같은 대화에서 입실 시각을 다시 주지 않고 되묻는다. | `09:07` |

11월 21일 검증용 Mac의 Claude CLI는 로그인이 필요했다. 실행기가 첫 질문 전에 중단되어 이번 Claude 대화 7건은 채점 대기다. ML-04는 같은 날 Chrome과 회원 DB에서 통과했다. 이전 공급자 실험 원본은 이 문서의 Git 리비전 `24846dc`에 보존했다. 기존 Claude 장기 기억 측정에서는 1,002턴의 동명이인 질문 3건 모두에 올바르게 답했다. [장기 기억 측정](../../audit/memory-reader/results.md)

```sh
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py target/release/moosuri --claude
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py target/release/moosuri --expect-output-limit
```

## 긴 대화와 한 번의 모델 요청 한도

4,096토큰을 예약한 합성 회원과 실제 모델 브로커로 아래 세 경우를 실행했다. 변경 전에는 세 번째 질문에서 오래된 대화까지 보내려다 답을 받을 수 없었다. 변경 후 세 경우가 통과했다. 원래 대화는 SQLite에서 확인하고, 공급자가 받은 내용은 전용 시험 HTTP 서버에서 대조했다.

| ID | 회원 조작과 결과 |
| --- | --- |
| MB-01 | 긴 첫 질문 뒤 두 번째 질문을 보낸다. 직전 질문·답과 새 질문이 함께 모델에 전달된다. |
| MB-02 | 셋째 질문을 보내면 가장 오래된 한 쌍만 모델 입력에서 빠진다. 최근 질문·답으로 새 답을 받고 화면과 저장 이력에는 세 턴이 모두 남는다. |
| MB-03 | 현재 입력만으로 이미 한도를 넘기는 질문을 보낸다. 길이 안내와 초안이 보이며 새로고침 뒤에도 초안이 남는다. 추가 모델 호출과 저장된 실행은 각각 0건이다. |

정확히 저장한 후속 지시의 경우 원래 질문과 새 지시를 보존하고, 들어갈 수 있는 이전 답변 접두부를 유효한 UTF-8로 전달한다. 필수 두 입력이 너무 길면 원래 접수증으로 다시 열 수 있다. [저장소 검사](../../crates/moosuri-store/src/personal_text_preview.rs)

```sh
python3 deployment/kiri/linux-vm-smoke/member-web-fullstack.py target/release/moosuri --history-budget
```
