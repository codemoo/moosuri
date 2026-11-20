# Kiri 회원 기능 시나리오

Chrome 시나리오는 정적 Kiri 웹 빌드에 로그인, 목록, 저장 및 재시도 응답을 합성해서 검증한다. 화면에서 버튼·링크·대화 입력을 조작하고 브라우저 방문 기록와 새로고침을 사용한다. Google 동의와 볼트 새 탭은 브라우저 수준에서 가로채며 루프백 외 네트워크 요청을 차단한다. Vitest는 화면 상태와 요청 계약을, Rust는 게이트웨이·회원 소켓·영속 저장 등의 계약을 확인한다. 실제 모델 답변 품질, 실제 Google 동의와 조회, 모바일 실기기, 운영 서버 재시작은 별도 수락이 필요하다.

## 실행

Python 3.10 이상, Playwright Python, 설치된 Google Chrome이 필요하다. 저장소 루트에서 실행한다. `--scenario B-CHAT-02` 또는 `--width 390`을 반복하여 실행 대상을 선택할 수 있다. `--headed`는 눈으로 화면을 보며 재현할 때 쓴다.

```sh
npm ci --ignore-scripts --prefix web
npm run build --prefix web
python3 deployment/kiri/linux-vm-smoke/member-ui-scenarios.py web/dist
npm test --prefix web
cargo test --locked -p moosuri personal_gateway
cargo test --locked -p moosuri-store personal_tasks
```

아래 표의 `B-`는 Chrome 시나리오 ID다. `V`는 표시한 웹 테스트 파일의 테스트 이름으로 찾고, `R`은 Rust 테스트 함수 이름으로 찾는다. 결과 집계에서는 같은 `B-`의 360·390·1440px 실행을 한 시나리오로 센다. 원래 대화 열기처럼 여러 단계를 밟는 경우에도 표의 한 행을 한 시나리오로 센다.

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
