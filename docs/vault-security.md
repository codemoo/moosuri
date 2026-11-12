# 볼트 보안·이전 계약

목표는 **다른 사용자와 서버 관리자가 source/DB를 열어도 암호·TOTP seed·개인 root key를 읽을 수 없게 하는 것**이다. 사용자만 아는 강한 vault master secret과 독립 recovery key로 기기에서 암호화한다. 계정 비밀번호와 vault master secret은 다른 값·다른 수명이다. admin escrow·서버 master key·계정 재설정으로 볼트 열기 기능은 없다.

## 방어 범위

| 위협 | 제공하는 경계·남는 한계 |
| --- | --- |
| DB/backup/source를 읽는 admin·다른 계정 | 서버에는 ciphertext·salt/KDF params·wrapped key·최소 metadata만 존재. 강한 비밀 없이는 복호화 불가. 약한 master password의 offline guessing 위험은 남음 |
| 일반 계정/API 공격 | tenant FK/auth/epoch·별도 cookie·origin·rate limit. 항목·encrypted summary도 다른 사용자에게 반환하지 않음 |
| admin password reset | 앱 로그인만 복구. 이전 vault master/recovery 없으면 볼트 복구 불가. 새 빈 vault 생성은 옛 암호문 복호화와 다름 |
| 악성 서버가 React/JS 교체 | 보통 웹/PWA만으로 방어 불가. 사용자가 unlock할 때 악성 JS가 키/평문을 훔칠 수 있음 |
| 악성 client update·OS/browser·clipboard | 해당 실행 환경을 신뢰해야 함. Worker/zeroize/CSP가 OS 악성코드 방어를 보장하지 않음 |
| 저장소 삭제·과거 backup 재생 | 독립 최신 journal과 key/envelope epoch로 앱 복원을 제한. 악성 operator의 삭제/서비스 거부를 막지는 못함 |

더 강한 admin 위협에는 backend와 독립된 signing key·배포권으로 검증/고정한 client 또는 독립 통제 origin을 제공하는 후속 선택지가 필요하다. 단순 별도 subdomain·PWA 설치·서버가 함께 제공한 hash만으로 독립성을 주장하지 않는다. 출시 문구에서 위 한계를 숨기지 않는다. 사용자 기기에서 확인한 서명/고정 버전만 실행하고 업데이트 거절 시험을 통과해야 강화 모드로 표시한다.

## 프로세스와 UI

Rust vault service는 별도 UID·DB·origin·cookie·journal·backup을 사용한다. agent 도구·LLM·관리자 UI에 decrypt API를 제공하지 않는다. React vault bundle과 전용 crypto Worker가 key를 다룬다. root key는 Worker에만 유지하고 UI는 선택한 항목의 필요 필드만 짧게 받는다. 검색 summary도 암호화하며 서버에서 비밀에 대한 full-text index를 만들지 않는다.

lock/logout/idle/account switch/worker generation 변경 시 root/임시 평문·object URL·clipboard timer를 정리한다. visibility 복귀·TOTP 복사·late response를 다시 검사한다. 브라우저/OS가 만든 모든 사본을 완전 소거한다고 말하지 않는다. service worker/private IndexedDB/localStorage에 키·암호·CSV 원본을 보관하지 않는다. account auth는 내부 검증을 거쳐 독립 vault session을 발급하고 master unlock과 합치지 않는다.

## 기존 v1 암호문 호환

| 요소 | 보존할 값·형식 |
| --- | --- |
| password KDF | Argon2id v0x13, memory64MiB, time3, lanes1, salt16bytes, output32bytes |
| 항목 암호 | AES-256-GCM, nonce12bytes, tag16bytes, OS cryptographic entropy |
| recovery | HKDF-SHA256, 기존 domain separation과 입력 길이 검증 |
| 문자열/직렬화 | normalization 없는 UTF-8 password, sorted canonical JSON/AAD, padded standard Base64 |
| key 수명 | 항목별 random item key·nonce, root wrap nonce ledger, immutable envelopes·epoch/generation |
| archive | authenticated manifest·순서/누락/변조 탐지, server 없이 복호화/이전 가능 |

primitive를 직접 구현하거나 cipher를 바꾸지 않는다. 원본 private key/암호문 대신 공개 가능한 합성 벡터로 기존 client→새 client와 역방향 교차 검증한다. Rust/WASM crypto를 선택할 때 Worker RPC·memory bound·Argon2 비용·zeroization·기기별 성능을 따로 검증한다. 새 tenant 정보가 AAD 의미를 바꾼다면 server가 몰래 재포장하지 않고 explicit version envelope + 사용자 로컬 rewrap migration을 사용한다.

## 반드시 유지할 기능

로그인/API-key/secret-note, 즐겨찾기·만료·검색·수정 충돌, 안전 난수 암호 생성, reveal/copy/자동 정리, 복구 키, password/root rotation, 독립 encrypted export/import를 유지한다. TOTP는 otpauth URI/Apple fallback, 알고리즘·digits·period, 남은 시간/갱신/복사 시 재계산을 검증한다. seed/code를 로그·검색 summary·assistant context에 넣지 않는다.

Apple CSV는 브라우저에서만 parse하고 BOM/CRLF/quote·byte/row cap·취소·중복·선택·partial commit을 처리한다. username filter는 일반 설정이며 특정 개인 ID를 공개 코드에 박지 않는다. CSV 원본을 서버에 보내거나 backup하지 않는다. 웹은 사용자 디스크의 원본 CSV를 임의로 지울 수 없으므로 가져온 결과와 로컬 원본 삭제 안내를 제공한다. 기존 사용자의 별도 파일 삭제 지시를 새 앱의 보장으로 표현하지 않는다.

key rotation은 durable candidate·immutable retained envelope·nonce reservation·epoch·lost ACK reconciliation을 유지한다. 과거 export 안의 wrapper는 이미 다운로드된 사본이므로 서버 rotation으로 없앨 수 없음을 표시한다. 새 archive import는 새 IDs/현재 key를 사용한다. key를 잃고 recovery도 없으면 admin이 해독하지 못한다.

## 다중 사용자·삭제·복원

vault DB도 `(tenant_id,item_id)`·envelope/nonce ledger 소유권을 강제한다. 계정 정지/삭제는 assistant journal/outbox와 vault journal의 idempotent 동기화로 처리한다. 전체 계정 삭제가 두 DB에서 원자적으로 끝났다고 거짓 표시하지 않는다. vault 요청은 현재 계정 상태를 검증하며 장애 시 새 요청을 열지 않는다. 삭제 ACK 전 pending 상태를 보인다. 실제 영수증과 최소 journal을 남겨 복원 때 옛 계정/envelope가 되살아나지 않게 한다.

백업은 암호문에도 outer encryption을 한다. outer key는 운영자가 서버 밖에 보관하는 별도 backup identity이며 사용자 vault master/recovery와 다르다. public recipient만 서버에 두고 key epoch/과거 identity 보관·journal 검증 자료·독립 backup target 접근을 [architecture](architecture.md)의 복구 키 계약으로 관리한다. DB snapshot과 독립 최신 journal/head·nonce/envelope 예약·key epoch를 대조한다. 복원은 새 private DB에 forward schema·최신 journal·session revoke·staged rotation 검증을 적용한 뒤 연다. journal/expected head/staged key state가 불명확하면 recovery_required이며 빈 journal을 자동 생성하지 않는다.

P8 gate는 암호 벡터·wrong secret·tamper·reorder/truncate·KDF 전 길이 거절·race/cancel·TOTP clock·CSV partial import·rotation kill/lost ACK·독립 archive 복원·두 사용자 API/Worker 격리·admin DB/log/network canary 검사다. 백업 복호화 시험은 합성 vault로 실행하고 실제 암호를 agent에게 넘기지 않는다.

## 비서 작업에서 credential 사용

독립 볼트가 비서와 연결되지 않는다는 뜻은 아니다. [credential-use](credential-use.md)의 trusted-device broker가 개인 opaque binding을 자동 재사용해 지원되는 typed 작업을 수행한다. 비서는 credential 참조/상태/허용된 결과와 영수증만 받으며 범용 복호화 API를 갖지 않는다. 첫 선택 또는 실제 모호함만 입력을 받고 매 실행 승인을 반복하지 않는다.

broker가 잠기거나 offline이면 같은 작업을 대기시키고 실행 슬롯을 반환한다. 원본 master/password/TOTP를 서버에 escrow하지 않는다. 별도 최소 scope server token을 사용자가 선택한 경우 서버가 쓸 수 있는 일반 connector credential이며 vault의 관리자 비열람 보장과 구별한다. 악성 서버 코드에 대한 강한 방어는 독립적으로 신뢰한 설치 client에서만 가능하다. CRED01~CRED05는 이 모드 구분·격리·수신인/대상 scope·rotation/revoke·lost ACK를 검증한다.

강화 모드의 비밀 격리와 작업 의도의 진위는 다르다. 기기가 local grant의 최대 범위를 소유하므로 서버가 grant를 확대할 수 없지만 v1은 그 범위 안에서 요청을 구성하는 서버를 신뢰한다. 독립 client 서명만으로 악성 서버의 in-scope 작업 위조까지 방어한다고 말하지 않는다. 실제 사용·online commitment/철회 ACK 경계는 [credential-use](credential-use.md)가 정본이다. 볼트와 지원되는 broker 연결은 v1 제공 범위이며 사용자가 설치/연결을 선택한다. 강화 client의 추가 위협 방어는 검증한 실행 방식에만 표시한다.
