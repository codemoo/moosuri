# 공식·커스텀 데이터 수집 연결

2026-11-13 사용자 요구 R29에 따른 수집 연결 구현 계약이다. 실제 실행 검증과 한계는 [현재 상태](status.md)를 따른다. 카톡 수집기는 공개 제품·이미지·설치 도구에
포함하지 않는다. 사용자가 별도로 운영하는 개인 수집기를 **커스텀 수집 연결**로 붙인다.
기존 카톡 서비스·cron·DB의 이동이나 변경은 이 계획에 포함하지 않는다.

## 지원 구분과 현재 상태

| 연결 | 배포·유지보수 책임 | 현재 구현 | 제한 |
| --- | --- | --- | --- |
| Gmail | 공식 Rust adapter | 개인 OAuth·허용 라벨/시작 날짜의 초기 수집과 history 동기화·명시적 삭제 | 실계정 승인/검증 별도, 본문·페이지 크기 제한 |
| Slack | 공식 Rust adapter | auth.test로 검증한 개인 user token, 채널 history·설정 기간의 parent/replies 재확인 | OAuth 앱 자동 생성 없음; 기간 밖 답글·삭제 전체 확인 불가 |
| 커스텀 HTTP 수집 | Rust 수신부, 외부 producer는 사용자 관리 | 개인 연결·만료 token·batch/status/receipt·실행 이력 | 외부 producer의 설치·운영·내용 진실성은 별도 |
| 개인 카톡 수집기 | 공개 배포 제외 | 기존 개인 환경 유지 | 개인 producer를 generic custom protocol로 변환하는 작업은 비공개로 별도 진행 |

‘공식’은 moosuri가 지원할 adapter 분류다. 서비스 제공자의 제휴·보증이나 현재 사용 가능을
뜻하지 않는다. 연결 UI와 에이전트는 분류, 설치된 adapter 버전, 실제 credential/권한,
설정 유효성, 검증 상태와 **현재 사용할 수 있는 기능**을 구별한다. 카톡 항목은 공개 adapter
catalog에 등록하지 않는다. 개인 연결의 표시 이름을 카톡으로 짓는 것은 가능하다.

기존 자료 API는 로그인 세션 기반 수동 입력으로 유지한다. 수집 연결 소유의 모음에는 수동 batch와
수동 disconnect를 적용할 수 없으며, 해당 연결의 API를 거친다. 수동 모음의 batch는 비어 있지 않은 최대32개 항목, local i64
cursor의 `+1`, incremental batch만 지원한다. `complete_snapshot=true`와 빈 batch는 거절한다.
이를 곧바로 Slack history cursor, 무변경 동기화, 외부 collector 인증으로 사용하지 않는다.

## 하나의 공통 경로

공식 adapter와 커스텀 수신기를 같은 서버 수집 경로로 정규화한다.

`공식 API / 외부 수집기 → 연결·실행 확인 → 정규화/검증 → 원문·색인·영수증·cursor 원자 저장 → 검색·기억의 출처`

- 공식 adapter는 Rust 코드로 배포한다. Gmail OAuth와 Slack 개인 user token·scope·rate limit·pagination·
  provider 오류를 처리하며 네트워크 IO를 SQLite transaction 밖에서 수행한다.
- 커스텀 연결은 **외부에서 moosuri로 HTTPS push**를 기본으로 한다. Mac이나 사설 수집기에
  포트를 공개할 필요가 없다. Tailscale-private/일반 HTTPS 모두 앱 인증을 유지한다.
- moosuri가 커스텀 shell, Python, Node, arbitrary URL fetch나 임의 plugin을 실행하지 않는다.
  개인 수집기의 구현 언어·원본 DB·설치 방식은 공개 서버와 독립이다. 공개 수신부는 Rust다.
- 연결당 최초 설정과 수집 전용 credential을 한 번 만들고 일상 batch마다 승인하지 않는다.
  메일 수집 권한과 메일 발송 권한은 별개이며 기존 발송 최종 승인 계약은 유지한다.

## 연결과 자료 추적

SQLite migration9와 core DTO에 저장한다. 각 연결은 해당 계정에 속한 별도 knowledge collection을 사용한다.
실행기는 공용이지만 연결·credential·cursor·원문·검색·이력은 사용자별이다.

| 객체 | 저장할 식별·상태 | 의미 |
| --- | --- | --- |
| SourceConnection | 서버가 정한 tenant/connection ID, official/custom kind, adapter/version, 표시 이름, immutable remote account/scope identity, config revision, credential epoch, paused/revoked 상태 | 연결 이름 변경으로 원본 namespace를 바꾸지 않음 |
| SourceSyncRun | server-issued run ID/lease generation, connection/config epoch, 시작·종료·최근 heartbeat, outcome, coverage, 고정 오류 코드 | 실행 이력은 실제 서버 관측과 외부 보고를 구별 |
| SourceBatchReceipt | connection/run/request ID, digest, local commit sequence, provider checkpoint before/after, committed_at, accepted/unchanged/rejected counts | 같은 request+digest는 같은 receipt, 다른 내용은 conflict |
| SourceProvenance | connection ID, immutable scope, external object/thread/parent ID, external version, 내부 source ID/revision, batch receipt, source observed_at와 server received_at | 답변 근거에서 원본→수집 연결→해당 batch를 추적 |

`(tenant, connection, immutable_scope, external_id)`가 원본 identity다. 다른 사용자나 다른
연결에서 같은 remote ID가 와도 합쳐지지 않는다. remote 계정/범위 변경은 새 scope 또는
명시적인 새 connection generation이며, 기존 자료의 origin을 조용히 바꾸지 않는다.
manual 모음은 그대로 유지하고 기존 원문을 새 연결 소유로 자동 전환하지 않는다.

**provider checkpoint와 local sequence는 별도다.** Gmail history ID, Slack pagination 등의
opaque checkpoint는 private 저장하고 로그·화면에는 값 대신 commit 시각/상태만 표시한다.
provider checkpoint는 tenant/connection/config scope에 묶고 자료·영수증과 같은 commit 뒤에
확정한다. 원격 네트워크 성공만으로 cursor를 진행하지 않는다.

에이전트 검색 결과와 원문 상세는 기존 source ID/revision 근거에 connection과 sync receipt를
연결한다. 사용자는 이름·마지막 확인·오류를 먼저 보고, 상세를 열면 범위·실행·batch를 본다.
데이터 건수·token·내부 ID를 기본 대화에 늘 노출하지 않는다.

## 커스텀 연결 인증과 프로토콜

1. 로그인한 사용자가 자기 연결 이름과 고정 수집 범위를 등록한다. 서버가 `custom_http` 종류와
   namespace를 정한다. client payload는 tenant, official 여부, 도구 권한을 지정할 수 없다.
2. 연결 하나의 ingest/status에만 쓸 수 있는 만료 가능한 난수 token을 한 번 표시한다.
   서버는 hash만 저장하고 Bearer header로 받는다. URL·일반 로그·채팅·모델에 넣지 않는다.
   회전/철회, 계정 비활성화/보안 epoch 변경, collection disconnect를 매 commit에서 확인한다.
   token은 대화/원문 읽기·메일 발송·관리자 기능·다른 연결에 접근할 수 없다.
3. versioned envelope에 request ID, config revision, run identity, source entries와 thread/parent metadata,
   expected local sequence와 opaque checkpoint를 넣는다. 크기/개수/버전/필드와 scope를 검사한다.
   원문·색인·의존 기억 철회·receipt·sequence·checkpoint가 한 transaction에서 확정된다.
4. 응답 유실은 같은 request와 같은 bytes로 재시도해 기존 receipt를 확인한다. 재시작한
   collector는 write-only scope의 상태/receipt로 cursor를 대조하고 자료 전체를 읽지 않는다.
   철회 후에는 예전 receipt 조회도 허용하지 않으며 자료를 다시 살리지 않는다.

관리자 역할로 다른 사용자의 연결·자료를 읽는 API는 제공하지 않는다. 이 token은 수집 전용
서버 인증이며 사용자 키 볼트가 아니다. 일시적인 앱 로그아웃과 장기 수집 grant 철회는 별도
동작으로 설명한다. 보안상 계정 epoch가 바뀌면 예전 token은 다시 설정해야 한다.

## 상태·실패·삭제

- ‘마지막 접속’(heartbeat), ‘마지막 자료 commit’, ‘마지막 범위 동기화 완료’를 따로 둔다.
  heartbeat는 자료/sequence/checkpoint를 바꾸지 않는다. 빈 성공은 별도 no-change receipt로
  기록하며 기존 빈 IngestBatch에 억지로 통과시키지 않는다.
- 전체 범위 완료는 실제 page coverage/최종 checkpoint가 있을 때만 표시한다. 커스텀 수집기가
  `success`를 보냈다는 사실은 **수집기 보고**이고, moosuri가 증명하는 것은 수신/commit이다.
  인증한 producer의 자료에도 거짓 내용·위조된 원본 시간이 있을 수 있으며 지시로 실행하지 않는다.
- 연결당 한 active run, bounded queue, lease/config generation을 사용한다. 오래된 성공/오류/
  heartbeat는 새 run 상태를 덮지 않는다. 429/retry-after·일시 네트워크·인증 필요·invalid batch·
  부분 완료·결과 불명·pause를 구별하고 제한된 재시도/backoff를 사용한다. polling에 LLM을 쓰지 않는다.
- 기본 delta는 빠진 항목을 삭제하지 않는다. `tombstones`의 같은 namespace 외부 ID만 삭제한다.
  기존 원문의 삭제는 journal에 남고 재수집을 억제한다. 한 번도 저장하지 않은 ID의 삭제는 no-op이며
  영구 억제 기록을 만들지 않는다. 동일 page의 다른 정상 자료는 계속 반영한다. full snapshot retirement는 검증된 완전한 범위/페이지와 generation이 모두 확정된
  별도 finalize 계약을 구현·검증한 뒤 활성화한다. partial/중단 snapshot은 삭제0이다.
- disconnect는 수집 중단+자료 보관/삭제 선택이다. 삭제 journal·현재 auth/config epoch를
  commit 순서에 묶고 late batch·오래된 backup이 삭제된 자료와 token을 되살리지 않게 한다.
- 원문 없는 고정 오류 코드·시각·카운터만 운영 진단으로 내보낸다. source text·credentials·
  원본 remote cursor·provider 원문 오류는 일반 로그에 남기지 않는다. 일반 실행 목록은64개, 참조되지 않는 receipt는1024개로 제한한다. 살아 있는 원문 revision이
  참조하는 receipt와 최소 run 정보는 보존해 오래된 자료의 출처 연결이 끊기지 않게 한다.

## 이용 방법과 지원 범위

사용자는 `/settings/sources`의 **자료 연결**에서 자기 연결만 관리한다. 개인 설정과 원문은 관리자
화면에 노출하지 않는다. 계정 전환은 진행 중 요청과 일회 표시 token을 폐기한다.

- **Gmail:** [메일 설정](mail.md)의 Google 연결을 먼저 마친 뒤 계정·라벨 ID·시작 날짜·주기를 선택한다.
  기본 연결만으로 자동 backfill하지 않는다. 최초 범위는 최근366일 이내이며, 본문 하나64KiB와
  page 전체256KiB 제한을 초과하면 checkpoint를 진행하지 않고 범위 조정이 필요한 오류를 표시한다.
  Gmail의 SPAM/TRASH도 명시한 라벨 범위에 포함할 수 있다. history 만료는 자동 삭제/전환 대신
  `checkpoint_invalid`로 알리고 **동기화 다시 시작**으로 같은 범위를 다시 읽는다.
- **Slack:** 필요한 history 권한이 있는 개인 user token과 채널 ID를 입력한다. 고정 `auth.test`에서
  workspace/user를 확인하며 다른 계정으로의 token 교체를 거절한다. 초기 범위와 스레드 재확인은
  최대90일이다. 설정 범위의 polling 완료는 `polled`이며 모든 삭제나 임의의 오래된 답글까지
  확인했다는 뜻이 아니다. 자동 OAuth 앱 설치나 Slack 쓰기 권한은 제공하지 않는다.
- **커스텀:** 이름/범위를 만들고 만료 가능한 수집 token을 발급한다. token은 한 번만 보이며
  계정 암호 변경·회전·철회 시 예전 token은 사용할 수 없다. 앱 로그아웃은 별도 동작이다.
- 일시정지는 진행 중 run을 fence한다. 해제는 **자료 보관** 또는 **자료 삭제**를 고른다.
  보관 후에도 같은 연결 화면에서 자료를 나중에 삭제할 수 있다. 원문 상세의 **수집 출처**에서
  연결·원본 thread/parent/version·실행과 저장 영수증을 확인한다.

수집기는5초마다 due 연결을 확인하고 LLM을 호출하지 않는다. 연결별 one-active-run, page budget,
300초 lease와 heartbeat, rate-limit/backoff로 작업을 제한한다. 정상 pagination은 partial로 이어가며
실패와 구별한다. 서버 재시작/lease 만료는 unknown으로 남긴다. 메일 발송 승인 계약은 그대로다.

## 외부 producer HTTP 프로토콜

모든 경로의 접두사는 `/api/v1/source-ingest`다. HTTPS와 `Authorization: Bearer <token>`을 사용하며
Cookie/Origin 또는 사용자 세션을 혼용하지 않는다. Tailscale도 앱 인증을 생략하지 않는다.

| 메서드·경로 | 동작 |
| --- | --- |
| `GET /status` | 자기 connection/config/local sequence/private checkpoint와 active run 확인 |
| `POST /runs` | 서버 run ID/generation 발급; 한 연결에서 동시에 하나만 실행 |
| `POST /heartbeat` | `{run_id,generation}` 생존 확인, 자료/sequence/checkpoint 변경 없음 |
| `POST /batches` | version1 envelope의 원문·metadata·명시적 tombstone·checkpoint 원자 반영 |
| `GET /receipts/{request_id}` | 자기 요청의 안전한 receipt 조회; 원문 읽기 권한은 없음 |
| `POST /finish` | run ID/generation·outcome·고정 error·선택 retry_after_seconds로 보고 |

`SourceBatch`는 `protocol_version`, UUID `request_id`, `config_revision`, `run_id`, `generation`,
`expected_sequence`, `checkpoint_before/after`, `entries`, `tombstones`, `metadata`,
`complete_snapshot:false`를 받는다. 빈 entries는 독립 no-change receipt를 만들 수 있지만
heartbeat는 아니다. 한 번에 upsert/delete 합계32개, decoded text256KiB, JSON512KiB까지다.
같은 요청은 **저장해 둔 동일 envelope**로 재시도한다. payload 변경은 conflict이며, 늦은
완료·옛 token·다른 계정의 receipt 조회는 허용하지 않는다. full snapshot은 계속 거절한다.

[작은 Rust producer 예제](../crates/moosuri-connectors/examples/custom_source_sender.rs)는 합성 note만
보내며, private state 파일에 envelope를 먼저 fsync하여 응답 유실 후 같은 요청을 재시도한다.
[source adapter README](../crates/moosuri-connectors/src/source/README.md)의 실행·복구 절차를 따른다.
개인 카톡 source/DB/설치 프로그램은 이 예제나 공개 패키지에 포함하지 않는다.

## 코드와 검증 추적

[OpenSpec](../openspec/changes/build-source-connections/tasks.md), [연결 추적표](../audit/source-connectors.tsv),
[검증 기록](reviews/2026-11-13-source-connections.md)을 함께 관리한다. 구현은 core/store/server/agent의
`source_connections.rs`, connectors의 `source/{gmail,slack,transport}.rs`, React의
`SourceConnections.tsx`와 `SourceProvenance.tsx`에 있다. 계획 검증기는 target/spec/evidence의 존재와
참조를 확인하며, 파일 존재를 runtime 품질이나 실계정 검증으로 대신하지 않는다.

합성 SQLite·HTTP·React와 Linux 검증은 실제 Google/Slack 계정 승인을 대신하지 않는다.
실계정 opt-in 검증과 운영 배포는 그 증거가 있을 때만 따로 기록한다.
