# 계정 관리

관리자 화면의 People에서 각 사람의 Manage를 열어 계정 정지·재개, 역할 변경,
임시 비밀번호 재발급과 모든 기기 로그아웃을 실행한다. 변경할 때 기존 세션과
background 실행 권한은 만료된다. 저장된 대화·자료는 유지하며 관리자가 열람하는
기능은 제공하지 않는다. 재개 후에는 새로 로그인하고 필요한 background 설정을
다시 활성화한다. 이전 작업이나 세션을 자동으로 되살리지 않는다.

새 임시 비밀번호는24시간 유효하고 처음 로그인하면 반드시 바꿔야 한다. 화면에
한 번만 표시하며 이메일은 보내지 않는다. 응답 유실 때 Retry action은 같은 요청의
영수증을 확인한다. 이미 발급한 암호는 다시 표시하지 않으므로 잃었다면 Reset
password로 새로 발급한다. 정지된 계정의 암호를 바꿔도 정지 상태를 유지한다.

본인의 역할·정지·암호를 관리자 동작으로 바꿀 수 없다. 본인 암호는 일반 암호 변경을
사용하고, 역할과 정지는 다른 관리자에게 맡긴다. 본인 기기의 전체 로그아웃은
가능하며 즉시 로그인 화면으로 돌아간다. 계정 내용이 바뀐 경우에는 최신 상태를
다시 보고 동작을 선택한다. 새 revision으로 임의 재시도하지 않는다.

변경 중 저장이 실패하면 해당 프로세스는 요청을 중단한다. 재시작은 새 auth epoch를
적용하고 영향을 받은 계정을 recovery_required로 잠근다. 성공한 것으로 추정하거나
웹 Resume로 잠금을 풀지 않는다. 중지한 설치의 명시적 로컬 계정 복구 절차를 따른다.
일반 password reset·operator recovery 모두 다른 최신 편집을 덮어쓰지 않도록
관리 revision을 올린다. 로컬 복구도 기존 suspended 상태를 없애지 않는다.

초대 메일 발송·구독 및 비용 정책은 이 단위의 구현 범위가 아니다. 실제 검증 수준은 [검증 기록](reviews/2026-11-13-account-management.md)과
[현재 상태](status.md)를 따른다.

## 영구 삭제와 볼트 전달

People의 Delete account에서 삭제 범위와 현재 revision을 검토한 뒤 Permanently
delete account를 실행한다. 다른 관리자가 대상 계정을 삭제할 수 있다. 본인 삭제는
거절하며 마지막 active admin을 없애는 동작도 거절한다. 정지된 계정도 삭제할 수
있다. 관리자 역할은 상대의 볼트 복호화·열람 권한을 만들지 않는다.

삭제는 독립 auth journal에 불투명 account ID/epoch와 삭제 표시를 fsync한 뒤
앱 계정을 즉시 정지·익명화하고 암호 hash·임시 암호·세션을 폐기한다. 재가입은
새 account/tenant를 사용한다. 삭제 표시는 operator password recovery나 Resume로
해제할 수 없다. journal과 DB 사이의 중단은 시작 시 삭제를 적용하고 readiness
전에 개인 자료 정리를 다시 수행한다. 이전 SQLite snapshot에 최신 journal을
적용해도 같은 삭제가 유지된다. journal 자체를 잃은 오래된 독립 복사본의 최신성을
보장하는 기능은 아니다.

개인 대화·원문·FTS·기억·학습·예약·브라우저 상태·Push·연결 credential·미디어와
첨부 BLOB을 해당 tenant에서 제거한다. 이 버전의 개인 파일은 SQLite BLOB이며
별도 사용자 filesystem home을 생성하지 않는다. 다른 사용자와 공용 provider
연결·공개 cache를 삭제하지 않는다. 최소 model 사용량 원장과 불투명한 외부 작업
ID/상태는 유지한다. 이미 dispatch한 외부 효과를 철회하거나 provider의 원격 자료를
삭제했다고 주장하지 않는다. 미확정 상태는 계속 미확정이며 삭제 뒤 credential을
다시 사용해 reconciliation하지 않는다.

개인 자료 정리가 실패해도 로그인은 즉시 차단되고 화면에는 cleanup pending이
남는다. 서버 재시작이나 같은 Retry action이 정리를 재시도한다. 계정 변경 성공
응답과 완전한 정리를 같은 상태로 표시하지 않는다.

별도 볼트 전달은 assistant의 durable outbox → 인증된 내부 adapter → vault의
자체 journal/DB transaction → 정확한 event ACK 순서다. 볼트 서비스가 약2초마다
한 event를 가져오며 삭제를 우선한다. 실패·응답 유실·재시작은 같은 event로
재시도한다. 삭제 시 해당 소유자의 세션·현재와 과거 DB ciphertext·후보·nonce
행을 제거하고 영구 owner tombstone을 남긴다. 오래된 grant/늦은 commit이나 더
높은 epoch도 삭제한 owner를 재활성화하지 못한다. 정지·암호 변경 등 일반 epoch
철회는 이전 vault 세션과 pending 후보를 막고 새 로그인만 허용한다.

People은 local cleanup과 별도 vault ACK를 각각 표시한다. 볼트가 없거나 꺼져
있으면 ACK는 pending이며 완료로 추정하지 않는다. vault의 fsync된 lifecycle
intent가 DB보다 앞선 중단은 시작 시 해당 단조로운 철회만 replay한다. 일반
ciphertext mutation의 모호한 journal gap은 기존대로 fail closed다.

완료는 현재 서비스의 논리적 삭제와 전달 ACK를 뜻한다. 암호화된 vault intent
journal·sealed checkpoint·이미 내보낸 backup의 과거 bytes, SQLite의 저장 매체
잔여 bytes와 사용자가 이미 받은 파일/clipboard는 보관 수명 또는 별도 폐기
대상이다. 물리 매체 secure erase를 약속하지 않는다. 복구 replay는 owner 삭제
표시를 유지하고 assistant의 최신 계정 검증이 새 접근을 거절한다.
