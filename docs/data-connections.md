# 공식·커스텀 데이터 수집 연결

2026-11-13 사용자 요구 R29에 따른 구현 계획이다. 카톡 수집기는 공개 제품·이미지·설치 도구에
포함하지 않는다. 사용자가 별도로 운영하는 개인 수집기를 **커스텀 수집 연결**로 붙인다.
기존 카톡 서비스·cron·DB의 이동이나 변경은 이 계획에 포함하지 않는다.

## 지원 구분과 현재 상태

| 연결 | 배포·유지보수 책임 | 현재 구현 | 다음 구현 |
| --- | --- | --- | --- |
| Gmail | 공식 지원 adapter, moosuri가 유지보수 | 개인 OAuth와 대화 중 조회·승인 후 발송 | 허용한 메일 범위의 자료 동기화 |
| Slack | 공식 지원 adapter, moosuri가 유지보수 | 미구현 | 공식 API의 개인 연결·channel/thread 범위 동기화 |
| 커스텀 HTTP 수집 | moosuri는 수신 프로토콜, 외부 수집기는 사용자 관리 | 미구현 | 사용자별 연결 등록·수집 전용 인증·batch 수신·상태/이력 |
| 개인 카톡 수집기 | 별도 비공개 프로젝트·별도 프로세스 | 기존 개인 환경만 사용 | 사용자가 generic custom protocol로 변환해 전송 |

‘공식’은 moosuri가 지원할 adapter 분류다. 서비스 제공자의 제휴·보증이나 현재 사용 가능을
뜻하지 않는다. 연결 UI와 에이전트는 분류, 설치된 adapter 버전, 실제 credential/권한,
설정 유효성, 검증 상태와 **현재 사용할 수 있는 기능**을 구별한다. 카톡 항목은 공개 adapter
catalog에 등록하지 않는다. 개인 연결의 표시 이름을 카톡으로 짓는 것은 가능하다.

현재 자료 API는 로그인 세션 기반 수동 입력이다. 비어 있지 않은 최대32개 항목, local i64
cursor의 `+1`, incremental batch만 지원한다. `complete_snapshot=true`와 빈 batch는 거절한다.
이를 곧바로 Slack history cursor, 무변경 동기화, 외부 collector 인증으로 사용하지 않는다.

## 하나의 공통 경로

공식 adapter와 커스텀 수신기를 같은 서버 수집 경로로 정규화한다.

`공식 API / 외부 수집기 → 연결·실행 확인 → 정규화/검증 → 원문·색인·영수증·cursor 원자 저장 → 검색·기억의 출처`

- 공식 adapter는 검토한 Rust 코드로 배포한다. Google/Slack OAuth·scope·rate limit·pagination·
  provider 오류를 처리하며 네트워크 IO를 SQLite transaction 밖에서 수행한다.
- 커스텀 연결은 **외부에서 moosuri로 HTTPS push**를 기본으로 한다. Mac이나 사설 수집기에
  포트를 공개할 필요가 없다. Tailscale-private/일반 HTTPS 모두 앱 인증을 유지한다.
- moosuri가 커스텀 shell, Python, Node, arbitrary URL fetch나 임의 plugin을 실행하지 않는다.
  개인 수집기의 구현 언어·원본 DB·설치 방식은 공개 서버와 독립이다. 공개 수신부는 Rust다.
- 연결당 최초 설정과 수집 전용 credential을 한 번 만들고 일상 batch마다 승인하지 않는다.
  메일 수집 권한과 메일 발송 권한은 별개이며 기존 발송 최종 승인 계약은 유지한다.

## 연결과 자료 추적

아래는 예정 필드이며 새 migration/DTO 구현 전까지 존재하는 API라고 표시하지 않는다.

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
3. versioned envelope에 request ID, config revision, run identity, source entries,
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
- 기본 delta는 빠진 항목을 삭제하지 않는다. delete event는 같은 namespace의 명시적 tombstone만
  적용한다. full snapshot retirement는 검증된 완전한 범위/페이지와 generation이 모두 확정된
  별도 finalize 계약을 구현·검증한 뒤 활성화한다. partial/중단 snapshot은 삭제0이다.
- disconnect는 수집 중단+자료 보관/삭제 선택이다. 삭제 journal·현재 auth/config epoch를
  commit 순서에 묶고 late batch·오래된 backup이 삭제된 자료와 token을 되살리지 않게 한다.
- 원문 없는 고정 오류 코드·시각·카운터만 운영 진단으로 내보낸다. source text·credentials·
  원본 remote cursor·provider 원문 오류는 일반 로그에 남기지 않는다. 기록 보관/정리도 bounded다.

## 구현 순서와 코드 좌표

[OpenSpec 작업 목록](../openspec/changes/build-source-connections/tasks.md)이 구현 backlog다.
[연결 추적표](../audit/source-connectors.tsv)는 종류/배포 범위·현재 상태·목표 코드·검증 연결을
담으며 Rust `xtask verify-plan`이 누락·중복·잘못된 완료 표기와 카톡 public target 재등장을 검사한다.
예정 파일을 만든 것만으로 아래 단계의 완료를 체크하지 않는다.

아래 Rust 경로는 repository의 `crates/moosuri-` 접두사를 생략한다. 정확한 경로는 연결 추적표에 둔다.

1. `core/src/source_connections.rs`, `store/src/source_connections.rs`, 새 SQLite migration:
   연결·scope·run/receipt/provenance 계약. 기존 `knowledge/sources.rs`의 batch 저장을 공통 commit
   함수로 추출하되 수동 입력 동작과 철회 계약을 유지한다.
2. `server/src/source_connections.rs`, `connectors/src/source/`:
   custom 등록/수집 token/ingest/status/receipt API, versioned 합성 fixture와 작은 외부 전송 예제.
   임의 실행 host가 아닌 독립 HTTP protocol. 실제 개인 카톡 연결은 별도 비공개 작업으로 진행한다.
3. `connectors/src/source/gmail.rs`, `.../slack.rs`, agent의 수집 supervisor:
   공식 API 연결·scope 설정·incremental sync·pagination·rate limit·reconnect. Gmail의 기존
   interactive OAuth를 수집 consent 없이 자동 backfill에 사용하지 않는다.
4. `web/src/SourceConnections.tsx`, `KnowledgeView`, source evidence DTO:
   공식/커스텀 추가, 이름/범위/마지막 성공/오류/재연결/일시정지, 상세 run/receipt와 원문 역추적.
   tenant/account 전환 시 늦은 결과와 일회 token을 폐기한다.
5. DATA01~DATA03·DATA06~DATA08과 source delete/forget·기존 chat evidence 회귀,
   독립 경계 검토·Linux 실제 HTTP/SQLite·제한된 opt-in provider 검증 후 각각 출시 상태를 갱신한다.

이 변경의 완료는 **계획·OpenSpec·전수 조사 target·Rust 계획 검증**까지다. 공식 자동 동기화,
외부 custom token/ingest API, 연결 관리 화면은 후속 구현 task이며 현재 동작한다고 표시하지 않는다.
